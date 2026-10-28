cmd_archival/libarchive/data_skip.o := riscv32-unknown-linux-musl-gcc -Wp,-MD,archival/libarchive/.data_skip.o.d  -std=gnu99 -Iinclude -Ilibbb  -include include/autoconf.h -D_GNU_SOURCE -DNDEBUG -D_LARGEFILE_SOURCE -D_LARGEFILE64_SOURCE -D_FILE_OFFSET_BITS=64 -DBB_VER='"1.35.0"' -Wall -Wshadow -Wwrite-strings -Wundef -Wstrict-prototypes -Wunused -Wunused-parameter -Wunused-function -Wunused-value -Wmissing-prototypes -Wmissing-declarations -Wno-format-security -Wdeclaration-after-statement -Wold-style-definition -finline-limit=0 -fno-builtin-strlen -fomit-frame-pointer -ffunction-sections -fdata-sections -fno-guess-branch-probability -funsigned-char -static-libgcc -falign-functions=1 -falign-jumps=1 -falign-labels=1 -falign-loops=1 -fno-unwind-tables -fno-asynchronous-unwind-tables -fno-builtin-printf -Os    -DKBUILD_BASENAME='"data_skip"'  -DKBUILD_MODNAME='"data_skip"' -c -o archival/libarchive/data_skip.o archival/libarchive/data_skip.c

deps_archival/libarchive/data_skip.o := \
  archival/libarchive/data_skip.c \
  /opt/riscv32_musl/sysroot/usr/include/stdc-predef.h \
  include/libbb.h \
    $(wildcard include/config/feature/shadowpasswds.h) \
    $(wildcard include/config/use/bb/shadow.h) \
    $(wildcard include/config/selinux.h) \
    $(wildcard include/config/feature/utmp.h) \
    $(wildcard include/config/locale/support.h) \
    $(wildcard include/config/use/bb/pwd/grp.h) \
    $(wildcard include/config/lfs.h) \
    $(wildcard include/config/feature/buffers/go/on/stack.h) \
    $(wildcard include/config/feature/buffers/go/in/bss.h) \
    $(wildcard include/config/extra/cflags.h) \
    $(wildcard include/config/variable/arch/pagesize.h) \
    $(wildcard include/config/feature/verbose.h) \
    $(wildcard include/config/feature/etc/services.h) \
    $(wildcard include/config/feature/ipv6.h) \
    $(wildcard include/config/feature/seamless/xz.h) \
    $(wildcard include/config/feature/seamless/lzma.h) \
    $(wildcard include/config/feature/seamless/bz2.h) \
    $(wildcard include/config/feature/seamless/gz.h) \
    $(wildcard include/config/feature/seamless/z.h) \
    $(wildcard include/config/float/duration.h) \
    $(wildcard include/config/feature/check/names.h) \
    $(wildcard include/config/feature/prefer/applets.h) \
    $(wildcard include/config/long/opts.h) \
    $(wildcard include/config/feature/pidfile.h) \
    $(wildcard include/config/feature/syslog.h) \
    $(wildcard include/config/feature/syslog/info.h) \
    $(wildcard include/config/warn/simple/msg.h) \
    $(wildcard include/config/feature/individual.h) \
    $(wildcard include/config/shell/ash.h) \
    $(wildcard include/config/shell/hush.h) \
    $(wildcard include/config/echo.h) \
    $(wildcard include/config/printf.h) \
    $(wildcard include/config/test.h) \
    $(wildcard include/config/test1.h) \
    $(wildcard include/config/test2.h) \
    $(wildcard include/config/kill.h) \
    $(wildcard include/config/killall.h) \
    $(wildcard include/config/killall5.h) \
    $(wildcard include/config/chown.h) \
    $(wildcard include/config/ls.h) \
    $(wildcard include/config/xxx.h) \
    $(wildcard include/config/route.h) \
    $(wildcard include/config/feature/hwib.h) \
    $(wildcard include/config/desktop.h) \
    $(wildcard include/config/feature/crond/d.h) \
    $(wildcard include/config/feature/setpriv/capabilities.h) \
    $(wildcard include/config/run/init.h) \
    $(wildcard include/config/feature/securetty.h) \
    $(wildcard include/config/pam.h) \
    $(wildcard include/config/use/bb/crypt.h) \
    $(wildcard include/config/feature/adduser/to/group.h) \
    $(wildcard include/config/feature/del/user/from/group.h) \
    $(wildcard include/config/ioctl/hex2str/error.h) \
    $(wildcard include/config/feature/editing.h) \
    $(wildcard include/config/feature/editing/history.h) \
    $(wildcard include/config/feature/tab/completion.h) \
    $(wildcard include/config/feature/editing/savehistory.h) \
    $(wildcard include/config/feature/username/completion.h) \
    $(wildcard include/config/feature/editing/vi.h) \
    $(wildcard include/config/feature/editing/save/on/exit.h) \
    $(wildcard include/config/pmap.h) \
    $(wildcard include/config/feature/show/threads.h) \
    $(wildcard include/config/feature/ps/additional/columns.h) \
    $(wildcard include/config/feature/topmem.h) \
    $(wildcard include/config/feature/top/smp/process.h) \
    $(wildcard include/config/pgrep.h) \
    $(wildcard include/config/pkill.h) \
    $(wildcard include/config/pidof.h) \
    $(wildcard include/config/sestatus.h) \
    $(wildcard include/config/unicode/support.h) \
    $(wildcard include/config/feature/mtab/support.h) \
    $(wildcard include/config/feature/clean/up.h) \
    $(wildcard include/config/feature/devfs.h) \
  include/platform.h \
    $(wildcard include/config/werror.h) \
    $(wildcard include/config/big/endian.h) \
    $(wildcard include/config/little/endian.h) \
    $(wildcard include/config/nommu.h) \
  /opt/riscv32_musl/sysroot/usr/include/limits.h \
  /opt/riscv32_musl/sysroot/usr/include/features.h \
  /opt/riscv32_musl/sysroot/usr/include/bits/alltypes.h \
  /opt/riscv32_musl/sysroot/usr/include/bits/limits.h \
  /opt/riscv32_musl/sysroot/usr/include/byteswap.h \
  /opt/riscv32_musl/sysroot/usr/include/stdint.h \
  /opt/riscv32_musl/sysroot/usr/include/bits/stdint.h \
  /opt/riscv32_musl/sysroot/usr/include/endian.h \
  /opt/riscv32_musl/sysroot/usr/include/stdbool.h \
  /opt/riscv32_musl/sysroot/usr/include/unistd.h \
  /opt/riscv32_musl/sysroot/usr/include/ctype.h \
  /opt/riscv32_musl/sysroot/usr/include/dirent.h \
  /opt/riscv32_musl/sysroot/usr/include/bits/dirent.h \
  /opt/riscv32_musl/sysroot/usr/include/errno.h \
  /opt/riscv32_musl/sysroot/usr/include/bits/errno.h \
  /opt/riscv32_musl/sysroot/usr/include/fcntl.h \
  /opt/riscv32_musl/sysroot/usr/include/bits/fcntl.h \
  /opt/riscv32_musl/sysroot/usr/include/inttypes.h \
  /opt/riscv32_musl/sysroot/usr/include/netdb.h \
  /opt/riscv32_musl/sysroot/usr/include/netinet/in.h \
  /opt/riscv32_musl/sysroot/usr/include/sys/socket.h \
  /opt/riscv32_musl/sysroot/usr/include/bits/socket.h \
  /opt/riscv32_musl/sysroot/usr/include/setjmp.h \
  /opt/riscv32_musl/sysroot/usr/include/bits/setjmp.h \
  /opt/riscv32_musl/sysroot/usr/include/signal.h \
  /opt/riscv32_musl/sysroot/usr/include/bits/signal.h \
  /opt/riscv32_musl/sysroot/usr/include/paths.h \
  /opt/riscv32_musl/sysroot/usr/include/stdio.h \
  /opt/riscv32_musl/sysroot/usr/include/stdlib.h \
  /opt/riscv32_musl/sysroot/usr/include/alloca.h \
  /opt/riscv32_musl/sysroot/usr/include/stdarg.h \
  /opt/riscv32_musl/sysroot/usr/include/stddef.h \
  /opt/riscv32_musl/sysroot/usr/include/string.h \
  /opt/riscv32_musl/sysroot/usr/include/strings.h \
  /opt/riscv32_musl/sysroot/usr/include/libgen.h \
  /opt/riscv32_musl/sysroot/usr/include/poll.h \
  /opt/riscv32_musl/sysroot/usr/include/bits/poll.h \
  /opt/riscv32_musl/sysroot/usr/include/sys/ioctl.h \
  /opt/riscv32_musl/sysroot/usr/include/bits/ioctl.h \
  /opt/riscv32_musl/sysroot/usr/include/bits/ioctl_fix.h \
  /opt/riscv32_musl/sysroot/usr/include/sys/mman.h \
  /opt/riscv32_musl/sysroot/usr/include/bits/mman.h \
  /opt/riscv32_musl/sysroot/usr/include/sys/resource.h \
  /opt/riscv32_musl/sysroot/usr/include/sys/time.h \
  /opt/riscv32_musl/sysroot/usr/include/sys/select.h \
  /opt/riscv32_musl/sysroot/usr/include/bits/resource.h \
  /opt/riscv32_musl/sysroot/usr/include/sys/stat.h \
  /opt/riscv32_musl/sysroot/usr/include/bits/stat.h \
  /opt/riscv32_musl/sysroot/usr/include/sys/types.h \
  /opt/riscv32_musl/sysroot/usr/include/sys/sysmacros.h \
  /opt/riscv32_musl/sysroot/usr/include/sys/wait.h \
  /opt/riscv32_musl/sysroot/usr/include/termios.h \
  /opt/riscv32_musl/sysroot/usr/include/bits/termios.h \
  /opt/riscv32_musl/sysroot/usr/include/time.h \
  /opt/riscv32_musl/sysroot/usr/include/sys/param.h \
  /opt/riscv32_musl/sysroot/usr/include/pwd.h \
  /opt/riscv32_musl/sysroot/usr/include/grp.h \
  /opt/riscv32_musl/sysroot/usr/include/shadow.h \
  /opt/riscv32_musl/sysroot/usr/include/mntent.h \
  /opt/riscv32_musl/sysroot/usr/include/sys/statfs.h \
  /opt/riscv32_musl/sysroot/usr/include/sys/statvfs.h \
  /opt/riscv32_musl/sysroot/usr/include/bits/statfs.h \
  /opt/riscv32_musl/sysroot/usr/include/utmp.h \
  /opt/riscv32_musl/sysroot/usr/include/utmpx.h \
  /opt/riscv32_musl/sysroot/usr/include/arpa/inet.h \
  include/xatonum.h \
  include/bb_archive.h \
    $(wildcard include/config/feature/tar/uname/gname.h) \
    $(wildcard include/config/feature/tar/long/options.h) \
    $(wildcard include/config/tar.h) \
    $(wildcard include/config/dpkg.h) \
    $(wildcard include/config/dpkg/deb.h) \
    $(wildcard include/config/feature/tar/gnu/extensions.h) \
    $(wildcard include/config/feature/tar/to/command.h) \
    $(wildcard include/config/feature/tar/selinux.h) \
    $(wildcard include/config/cpio.h) \
    $(wildcard include/config/rpm2cpio.h) \
    $(wildcard include/config/rpm.h) \
    $(wildcard include/config/feature/ar/create.h) \
    $(wildcard include/config/feature/ar/long/filenames.h) \
    $(wildcard include/config/zcat.h) \

archival/libarchive/data_skip.o: $(deps_archival/libarchive/data_skip.o)

$(deps_archival/libarchive/data_skip.o):
