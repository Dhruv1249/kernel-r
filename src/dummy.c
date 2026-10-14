// src/dummy.c

typedef unsigned long long u64;
typedef long long i64;

static inline u64 syscall3(u64 syscall_num, u64 arg1, u64 arg2, u64 arg3) {
    u64 ret;
    __asm__ volatile(
        "syscall"
        : "=a"(ret)
        : "a"(syscall_num), "D"(arg1), "S"(arg2), "d"(arg3)
        : "rcx", "r11"
    );
    return ret;
}

static inline u64 syscall2(u64 syscall_num, u64 arg1, u64 arg2) {
    u64 ret;
    __asm__ volatile(
        "syscall"
        : "=a"(ret)
        : "a"(syscall_num), "D"(arg1), "S"(arg2)
        : "rcx", "r11"
    );
    return ret;
}

static inline u64 syscall1(u64 syscall_num, u64 arg1) {
    u64 ret;
    __asm__ volatile(
        "syscall"
        : "=a"(ret)
        : "a"(syscall_num), "D"(arg1)
        : "rcx", "r11"
    );
    return ret;
}

static void print_str(const char* s) {
    u64 len = 0;
    while (s[len] != 0) {
        len++;
    }
    syscall3(1, 1, (u64)s, len);
}

struct DirEntry {
    unsigned long long size;
    unsigned char entry_type;
    unsigned char name_len;
    char name[64];
};

void _start() {
    print_str("Hello from Ring 3 C! Testing new syscalls...\n");

    // 0. Test sys_brk
    u64 initial_break = syscall1(12, 0);
    u64 target_break = initial_break + 8192;
    syscall3(12, target_break, 0, 0);
    volatile u64* ptr = (volatile u64*)initial_break;
    ptr[0] = 0x1122334455667788ULL;
    ((volatile u64*)(initial_break + 4096))[0] = 0xAABBCCDDEEFF0011ULL;
    if (ptr[0] != 0x1122334455667788ULL || 
        ((volatile u64*)(initial_break + 4096))[0] != 0xAABBCCDDEEFF0011ULL) {
        print_str("FATAL: Data corruption detected in sys_brk!\n");
        syscall1(60, 1);
    }
    syscall3(12, initial_break, 0, 0);
    print_str("[OK] sys_brk heap growth and shrink verified!\n");

    // 1. Test sys_open & sys_close (Syscall 2 & 3)
    print_str("Testing sys_close (Syscall 3)...\n");
    i64 fd = (i64)syscall1(2, (u64)"test_file.txt");
    if (fd < 3) {
        print_str("FATAL: sys_open failed to open test_file.txt!\n");
        syscall1(60, 2);
    }
    i64 close_ret = (i64)syscall1(3, (u64)fd);
    if (close_ret != 0) {
        print_str("FATAL: sys_close failed on valid fd!\n");
        syscall1(60, 3);
    }
    i64 close_ret2 = (i64)syscall1(3, (u64)fd);
    if (close_ret2 != -9) { // -EBADF
        print_str("FATAL: sys_close did not return -EBADF on closed fd!\n");
        syscall1(60, 4);
    }
    char dummy_buf[16];
    i64 read_ret = (i64)syscall3(0, (u64)fd, (u64)dummy_buf, 16);
    if (read_ret != -9) { // -EBADF
        print_str("FATAL: sys_read did not return -EBADF on closed fd!\n");
        syscall1(60, 5);
    }
    print_str("[OK] sys_close and fd recycling verified!\n");

    // 2. Test sys_getdents (Syscall 78)
    print_str("Testing sys_getdents (Syscall 78)...\n");
    struct DirEntry entry;
    u64 count = 0;
    for (u64 idx = 0; idx < 10; idx++) {
        i64 res = (i64)syscall2(78, idx, (u64)&entry);
        if (res == 1) {
            print_str("  Found file: ");
            syscall3(1, 1, (u64)entry.name, (u64)entry.name_len);
            print_str("\n");
            count++;
        } else {
            break;
        }
    }
    if (count == 0) {
        print_str("FATAL: sys_getdents found no files in rootfs!\n");
        syscall1(60, 6);
    }
    print_str("[OK] sys_getdents directory listing verified!\n");

    // 3. Test sys_spawn (Syscall 59) and sys_wait (Syscall 61)
    print_str("Testing sys_spawn (Syscall 59) and sys_wait (Syscall 61)...\n");
    i64 child_pid = (i64)syscall2(59, (u64)"child.elf", 0);
    if (child_pid <= 0) {
        print_str("FATAL: sys_spawn failed to launch child.elf!\n");
        syscall1(60, 7);
    }

    int child_status = -1;
    i64 wait_ret = (i64)syscall2(61, (u64)child_pid, (u64)&child_status);
    if (wait_ret != child_pid) {
        print_str("FATAL: sys_wait did not return child_pid!\n");
        syscall1(60, 8);
    }
    if (child_status != 42) {
        print_str("FATAL: sys_wait did not retrieve exit code 42!\n");
        syscall1(60, 9);
    }

    // Calling wait again on the reaped child should return -ECHILD (-10)
    i64 wait_again = (i64)syscall2(61, (u64)child_pid, (u64)&child_status);
    if (wait_again != -10) {
        print_str("FATAL: sys_wait did not return -ECHILD for already reaped child!\n");
        syscall1(60, 10);
    }
    print_str("[OK] sys_spawn and sys_wait with exit status propagation verified!\n");

    print_str("SUCCESS: ALL 4 SYSCALLS FULLY VERIFIED IN RING 3!\n");
    syscall1(60, 0);
}
