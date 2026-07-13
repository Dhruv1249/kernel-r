// src/dummy.c

typedef unsigned long long u64;

// Helper function to invoke a syscall with 3 arguments (e.g., sys_write)
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

// Helper function to invoke a syscall with 1 argument (e.g., sys_exit)
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

void _start() {

    const char* str = "Hello from Ring 3 C! From inside the intiramfs tarball elf\n";
    syscall3(1, 1, (u64)str, 59);
    syscall1(60, 0);
    while (1) {}
}
