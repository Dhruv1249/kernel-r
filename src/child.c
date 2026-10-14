// src/child.c
typedef unsigned long long u64;

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
    const char* msg = "[Child] Hello from spawned child process! Exiting with code 42...\n";
    syscall3(1, 1, (u64)msg, 66);
    syscall1(60, 42); // Exit with code 42
}
