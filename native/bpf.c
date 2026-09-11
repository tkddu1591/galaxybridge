/* Original thin bindings to Apple's system BPF headers. No USB parsing here. */
#include <sys/types.h>
#include <sys/ioctl.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/resource.h>
#include <sys/acl.h>
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
#include <limits.h>

static int gb_acl_check(int fd) {
    acl_t acl = acl_get_fd_np(fd, ACL_TYPE_EXTENDED);
    /* Apple's filesec API reports ENOENT when this valid descriptor has no ACL. */
    if (acl == NULL) return errno == ENOENT ? 0 : -1;
    if (acl_valid(acl) < 0) {
        int error = errno; acl_free(acl); errno = error; return -1;
    }
    acl_entry_t entry;
    int key = ACL_FIRST_ENTRY;
    while (acl_get_entry(acl, key, &entry) == 0) {
        acl_tag_t tag;
        if (acl_get_tag_type(entry, &tag) < 0) {
            int error = errno; acl_free(acl); errno = error; return -1;
        }
        if (tag != ACL_EXTENDED_DENY) {
            acl_free(acl); errno = EPERM; return -1;
        }
        key = ACL_NEXT_ENTRY;
    }
    /* Darwin reports EINVAL after the last entry of a valid ACL. */
    int error = errno;
    acl_free(acl);
    if (error != EINVAL) { errno = error; return -1; }
    return 0;
}

/* State and executable files are checked through already-open descriptors. */
int gb_file_check(int fd, uid_t owner, mode_t mode) {
    struct stat metadata;
    if (fstat(fd, &metadata) < 0) return -1;
    if (!S_ISREG(metadata.st_mode) || metadata.st_uid != owner ||
        metadata.st_nlink != 1 || (metadata.st_mode & 07777) != mode) {
        errno = EPERM; return -1;
    }
    return gb_acl_check(fd);
}

int gb_directory_check(int fd) {
    struct stat metadata;
    if (fstat(fd, &metadata) < 0) return -1;
    if (!S_ISDIR(metadata.st_mode) || metadata.st_uid != 0 || (metadata.st_mode & 0022) != 0) {
        errno = EPERM; return -1;
    }
    return gb_acl_check(fd);
}

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

/* Preserve Rust's exec-error pipe until exec, but close all other inherited
 * capabilities at exec. Actually closing unknown descriptors here could make
 * Command::spawn falsely report success after a later pre-exec failure. */
int gb_worker_descriptors(int source_fd, int ceiling) {
    if (ceiling < 4 || ceiling > 1048576 || source_fd >= ceiling) {
        errno = EINVAL; return -1;
    }
    for (int fd = 3; fd < ceiling; ++fd) {
        int flags = fcntl(fd, F_GETFD);
        if (flags < 0) {
            if (errno == EBADF) continue;
            return -1;
        }
        if (fcntl(fd, F_SETFD, flags | FD_CLOEXEC) < 0) return -1;
    }
    if (source_fd >= 0) {
        if (source_fd != 3 && dup2(source_fd, 3) < 0) return -1;
        if (fcntl(3, F_SETFD, 0) < 0) return -1;
    }
    return 0;
}

int gb_inspection_prepare(int ceiling) {
    if (setsid() < 0) return -1;
    if (gb_worker_descriptors(-1, ceiling) < 0) return -1;
    struct rlimit core = {0, 0}, files = {256, 256};
    if (setrlimit(RLIMIT_CORE, &core) < 0 || setrlimit(RLIMIT_NOFILE, &files) < 0) return -1;
    return 0;
}

/* Called only in the pre-exec child. Descriptor, resource and credential
 * operations use fixed stack values and make no allocations or lookups. */
int gb_worker_prepare(int source_fd, int ceiling, uid_t uid, gid_t gid) {
    if (geteuid() != 0 || uid < 60000 || uid > 64999 || gid < 60000 || gid > 64999) {
        errno = EPERM; return -1;
    }
    if (setsid() < 0) return -1;
    if (gb_worker_descriptors(source_fd, ceiling) < 0) return -1;
    struct rlimit core = {0, 0}, files = {256, 256}, processes = {16, 16};
    if (setrlimit(RLIMIT_CORE, &core) < 0 || setrlimit(RLIMIT_NOFILE, &files) < 0 ||
        setrlimit(RLIMIT_NPROC, &processes) < 0) return -1;
    if (setgroups(0, NULL) < 0 || setgid(gid) < 0 || setuid(uid) < 0) return -1;
    if (getuid() != uid || geteuid() != uid || getgid() != gid || getegid() != gid) {
        errno = EPERM; return -1;
    }
    /* Darwin may report the effective group as its sole group-list entry. */
    gid_t groups[NGROUPS_MAX];
    int count = getgroups(NGROUPS_MAX, groups);
    if (count < 0) return -1;
    if (count > 1 || (count == 1 && groups[0] != gid)) { errno = EPERM; return -1; }
    /* A saved root credential must not survive the drop. */
    if (setgid(0) == 0 || errno != EPERM) { errno = EPERM; return -1; }
    if (setuid(0) == 0 || errno != EPERM) { errno = EPERM; return -1; }
    return 0;
}
