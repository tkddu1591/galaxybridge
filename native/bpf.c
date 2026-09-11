/* Original thin bindings to Apple's system BPF headers. No USB parsing here. */
#include <sys/types.h>
#include <sys/ioctl.h>
#include <sys/socket.h>
#include <net/if.h>
#include <net/bpf.h>
#include <fcntl.h>
#include <unistd.h>
#include <grp.h>
#include <errno.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

int gb_bpf_open(const char *name, unsigned int *size) {
    int fd = -1;
    for (int i = 0; i < 256; ++i) {
        char path[32];
        snprintf(path, sizeof(path), "/dev/bpf%d", i);
        fd = open(path, O_RDWR | O_CLOEXEC | O_NONBLOCK);
        if (fd >= 0) break;
        if (errno != EBUSY) return -1;
    }
    if (fd < 0) return -1;
    struct ifreq request;
    memset(&request, 0, sizeof(request));
    if (strlcpy(request.ifr_name, name, sizeof(request.ifr_name)) >= sizeof(request.ifr_name)) {
        close(fd); errno = EINVAL; return -1;
    }
    unsigned int one = 1, zero = 0;
    *size = 262144;
    if (ioctl(fd, BIOCSBLEN, size) < 0 || ioctl(fd, BIOCSETIF, &request) < 0 ||
        ioctl(fd, BIOCIMMEDIATE, &one) < 0 || ioctl(fd, BIOCSSEESENT, &zero) < 0 ||
        ioctl(fd, BIOCSHDRCMPLT, &one) < 0 || ioctl(fd, BIOCGBLEN, size) < 0) {
        int error = errno; close(fd); errno = error; return -1;
    }
    return fd;
}

void gb_bpf_layout(uint32_t result[4]) {
    result[0] = (uint32_t)offsetof(struct bpf_hdr, bh_caplen);
    result[1] = (uint32_t)offsetof(struct bpf_hdr, bh_datalen);
    result[2] = (uint32_t)offsetof(struct bpf_hdr, bh_hdrlen);
    result[3] = BPF_ALIGNMENT;
}

/* Called only in the pre-exec child; every call below is async-signal-safe. */
int gb_worker_prepare(int source_fd) {
    if (source_fd >= 0) {
        if (source_fd != 3 && dup2(source_fd, 3) < 0) return -1;
        if (fcntl(3, F_SETFD, 0) < 0) return -1;
    }
    if (setgroups(0, NULL) < 0 || setgid((gid_t)-2) < 0 || setuid((uid_t)-2) < 0) return -1;
    if (getuid() != (uid_t)-2 || geteuid() != (uid_t)-2) { errno = EPERM; return -1; }
    return 0;
}
