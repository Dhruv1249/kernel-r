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
    u64 initial_break = syscall1(12,0);
    u64 target_break = initial_break + 8192;
    syscall3(12, target_break, 0, 0);
    volatile u64* ptr = (volatile u64*)initial_break;
    ptr[0] = 0x1122334455667788ULL;
    ((volatile u64*)(initial_break + 4096))[0] = 0xAABBCCDDEEFF0011ULL;
    if (ptr[0] != 0x1122334455667788ULL || 
        ((volatile u64*)(initial_break + 4096))[0] != 0xAABBCCDDEEFF0011ULL) {
        syscall3(1, 1, (u64)"Data corruption detected!\n", 32);
        syscall1(60, 0);
    }
    syscall3(12, initial_break, 0, 0);
    syscall3(1, 1, (u64)"SUCCESS: NO Data corruption detected!\n", 32);
    syscall1(60, 0);
}
