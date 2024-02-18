#include <errno.h>
#include <unistd.h>
#include <sys/socket.h>
#include <string.h>
#include <stdlib.h>
#include <stdio.h>

#define NO_EINTR(command) \
  do {} while ((command) == -1 && errno == EINTR)

#define PASSERT(command) \
  if (!(command))             \
  {                           \
    perror("texpresso_fork_with_channel failure: " #command); \
    abort();                  \
  }

static void send_child_fd(int channel, int32_t pid, int child)
{
  ssize_t sent;
  char buffer[4] = "CHLD";
  NO_EINTR(sent = write(channel, buffer, 4));
  PASSERT(sent == 4);

  char msg_control[CMSG_SPACE(1 * sizeof(int))] = {0,};
  struct iovec iov = { .iov_base = &pid, .iov_len = 4 };
  struct msghdr msg = {
    .msg_iov = &iov, .msg_iovlen = 1,
    .msg_controllen = CMSG_SPACE(1 * sizeof(int)),
  };
  msg.msg_control = &msg_control;

  struct cmsghdr *cm = CMSG_FIRSTHDR(&msg);
  cm->cmsg_level = SOL_SOCKET;
  cm->cmsg_type = SCM_RIGHTS;
  cm->cmsg_len = CMSG_LEN(1 * sizeof(int));

  int *fds0 = (int*)CMSG_DATA(cm);
  fds0[0] = child;

  NO_EINTR(sent = sendmsg(channel, &msg, 0));
  PASSERT(sent == 4);
}

int texpresso_fork_with_channel(int fd)
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

  // Send socket and update channel in child
  if (child == 0)
  {
    PASSERT(dup2(sockets[1], fd) != -1);
  }
  else
  {
    send_child_fd(fd, child, sockets[0]);
  }
  PASSERT(close(sockets[0]) == 0);

  // Release temporary socket
  PASSERT(close(sockets[1]) == 0);

  return child;
}
