#include <errno.h>
#include <unistd.h>
#include <sys/socket.h>
#include <string.h>
#include <stdlib.h>
#include <stdio.h>
#include <signal.h>

#define NO_EINTR(command) \
  do {} while ((command) == -1 && errno == EINTR)

#define PASSERT(command) \
  if (!(command))             \
  {                           \
    perror("texpresso_fork_with_channel failure: " #command); \
    abort();                  \
  }

static void send_child_fd(int chan_fd, int32_t pid, uint32_t time, int child_fd)
{
  ssize_t sent;
  char msg_control[CMSG_SPACE(1 * sizeof(int))] = {0,};
  struct iovec iov[3] = {
    { .iov_base = "CHLD", .iov_len = 4 },
    { .iov_base = &time, .iov_len = 4 },
    { .iov_base = &pid, .iov_len = 4 },
  };
  struct msghdr msg = {
    .msg_iov = iov, .msg_iovlen = 3,
    .msg_controllen = CMSG_SPACE(1 * sizeof(int)),
  };
  msg.msg_control = &msg_control;

  struct cmsghdr *cm = CMSG_FIRSTHDR(&msg);
  cm->cmsg_level = SOL_SOCKET;
  cm->cmsg_type = SCM_RIGHTS;
  cm->cmsg_len = CMSG_LEN(1 * sizeof(int));

  int *fds0 = (int*)CMSG_DATA(cm);
  fds0[0] = child_fd;

  NO_EINTR(sent = sendmsg(chan_fd, &msg, 0));
  PASSERT(sent == 12);
}

int texpresso_fork_with_channel(int fd, uint32_t time)
{
  // Ignore SIGCHLD to simplify process management 
  static int signal_setup = 0;
  if (signal_setup == 0)
  {
    PASSERT(signal(SIGCHLD, SIG_IGN) != SIG_ERR);
    signal_setup = 1;
  }

  int sockets[2];
  
  // Create socket
  PASSERT(socketpair(PF_UNIX, SOCK_STREAM, 0, sockets) == 0);

  // Fork
  pid_t child;
  PASSERT((child = fork()) != -1);

  if (child == 0)
  {
    // In child: replace channel with new socket
    PASSERT(dup2(sockets[1], fd) != -1);
  }
  else
  {
    // In parent: send other end of new socket to driver
    send_child_fd(fd, child, time, sockets[0]);
    char answer[4];
    int recvd;
    NO_EINTR(recvd = read(fd, answer, 4));
    PASSERT(recvd == 4 && 
            answer[0] == 'D' && answer[1] == 'O' &&
            answer[2] == 'N' && answer[3] == 'E');
  }
  PASSERT(close(sockets[0]) == 0);

  // Release temporary socket
  PASSERT(close(sockets[1]) == 0);

  return child;
}
