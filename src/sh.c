// src/sh.c - Interactive Ring-3 Shell for kernel-r

typedef unsigned long long u64;
typedef long long i64;

static inline u64 syscall3(u64 num, u64 a1, u64 a2, u64 a3) {
    u64 ret;
    __asm__ volatile(
        "syscall"
        : "=a"(ret)
        : "a"(num), "D"(a1), "S"(a2), "d"(a3)
        : "rcx", "r11"
    );
    return ret;
}

static inline u64 syscall2(u64 num, u64 a1, u64 a2) {
    u64 ret;
    __asm__ volatile(
        "syscall"
        : "=a"(ret)
        : "a"(num), "D"(a1), "S"(a2)
        : "rcx", "r11"
    );
    return ret;
}

static inline u64 syscall1(u64 num, u64 a1) {
    u64 ret;
    __asm__ volatile(
        "syscall"
        : "=a"(ret)
        : "a"(num), "D"(a1)
        : "rcx", "r11"
    );
    return ret;
}

static u64 strlen(const char* s) {
    u64 len = 0;
    while (s[len] != 0) {
        len++;
    }
    return len;
}

static int strcmp(const char* s1, const char* s2) {
    while (*s1 && (*s1 == *s2)) {
        s1++;
        s2++;
    }
    return *(const unsigned char*)s1 - *(const unsigned char*)s2;
}

static void print(const char* s) {
    syscall3(1, 1, (u64)s, strlen(s));
}

static void print_char(char c) {
    syscall3(1, 1, (u64)&c, 1);
}

static void print_int(i64 n) {
    if (n < 0) {
        print_char('-');
        n = -n;
    }
    if (n == 0) {
        print_char('0');
        return;
    }
    char buf[24];
    int i = 0;
    while (n > 0) {
        buf[i++] = '0' + (n % 10);
        n /= 10;
    }
    for (int j = i - 1; j >= 0; j--) {
        print_char(buf[j]);
    }
}

static int readline(char* buf, int max_len) {
    int idx = 0;
    while (1) {
        char c = 0;
        i64 n = (i64)syscall3(0, 0, (u64)&c, 1);
        if (n <= 0) {
            continue;
        }

        // Enter key: '\r' or '\n'
        if (c == '\r' || c == '\n') {
            print("\n");
            buf[idx] = '\0';
            return idx;
        }

        // Backspace: '\b' or 0x7F
        if (c == '\b' || c == 0x7f) {
            if (idx > 0) {
                idx--;
                print("\b \b");
            }
            continue;
        }

        // Printable ASCII
        if (c >= 32 && c <= 126) {
            if (idx < max_len - 1) {
                buf[idx++] = c;
                print_char(c);
            }
        }
    }
}

static int parse_args(char* line, char* argv[], int max_args) {
    int argc = 0;
    char* p = line;

    while (*p && argc < max_args - 1) {
        while (*p == ' ' || *p == '\t') {
            p++;
        }
        if (*p == '\0') {
            break;
        }

        argv[argc++] = p;

        while (*p && *p != ' ' && *p != '\t') {
            p++;
        }

        if (*p != '\0') {
            *p = '\0';
            p++;
        }
    }
    argv[argc] = 0;
    return argc;
}

struct DirEntry {
    unsigned long long size;
    unsigned char entry_type;
    unsigned char name_len;
    char name[64];
};

static void cmd_help() {
    print("kernel-r Shell v0.1\n");
    print("Available built-in commands:\n");
    print("  help         - Display this help message\n");
    print("  echo [args]  - Print arguments to stdout\n");
    print("  clear        - Clear the terminal screen\n");
    print("  ls           - List files in initramfs\n");
    print("  cat <file>   - Display contents of a file\n");
    print("  pwd          - Print current working directory\n");
    print("  cd <dir>     - Change directory\n");
    print("  exit         - Exit shell\n");
    print("External binaries:\n");
    print("  <program>    - Spawn and execute an ELF binary (e.g. dummy.elf, child.elf)\n");
}

static void cmd_echo(int argc, char* argv[]) {
    for (int i = 1; i < argc; i++) {
        print(argv[i]);
        if (i + 1 < argc) {
            print(" ");
        }
    }
    print("\n");
}

static void cmd_clear() {
    print("\x1b[2J\x1b[H");
}

static void cmd_pwd() {
    print("/\n");
}

static void cmd_cd(int argc, char* argv[]) {
    if (argc < 2) {
        return;
    }
    print("cd: read-only flat TarFS mounted at /\n");
}

static void cmd_ls() {
    struct DirEntry entry;
    for (u64 idx = 0; idx < 100; idx++) {
        i64 res = (i64)syscall2(78, idx, (u64)&entry);
        if (res == 1) {
            print("  ");
            print_int((i64)entry.size);
            print(" bytes\t");
            syscall3(1, 1, (u64)entry.name, (u64)entry.name_len);
            print("\n");
        } else {
            break;
        }
    }
}

static void cmd_cat(int argc, char* argv[]) {
    if (argc < 2) {
        print("Usage: cat <filename>\n");
        return;
    }
    i64 fd = (i64)syscall1(2, (u64)argv[1]);
    if (fd < 0) {
        print("cat: ");
        print(argv[1]);
        print(": No such file or directory\n");
        return;
    }
    char buf[128];
    while (1) {
        i64 bytes = (i64)syscall3(0, (u64)fd, (u64)buf, sizeof(buf));
        if (bytes <= 0) {
            break;
        }
        syscall3(1, 1, (u64)buf, (u64)bytes);
    }
    syscall1(3, (u64)fd); // sys_close
}

static void cmd_exec(const char* name) {
    i64 pid = (i64)syscall2(59, (u64)name, 0);
    if (pid < 0) {
        print("sh: command not found: ");
        print(name);
        print("\n");
        return;
    }
    int status = 0;
    syscall2(61, (u64)pid, (u64)&status);
    if (status != 0) {
        print("[Process exited with code ");
        print_int(status);
        print("]\n");
    }
}

void _start() {
    print("\n");
    print("============================================\n");
    print("        Welcome to kernel-r Shell!          \n");
    print("    Type 'help' for a list of commands.     \n");
    print("============================================\n\n");

    char line_buf[256];
    char* argv[16];

    while (1) {
        print("kernel-r> ");
        int len = readline(line_buf, sizeof(line_buf));
        if (len == 0) {
            continue;
        }

        int argc = parse_args(line_buf, argv, 16);
        if (argc == 0) {
            continue;
        }

        if (strcmp(argv[0], "help") == 0) {
            cmd_help();
        } else if (strcmp(argv[0], "echo") == 0) {
            cmd_echo(argc, argv);
        } else if (strcmp(argv[0], "clear") == 0) {
            cmd_clear();
        } else if (strcmp(argv[0], "pwd") == 0) {
            cmd_pwd();
        } else if (strcmp(argv[0], "cd") == 0) {
            cmd_cd(argc, argv);
        } else if (strcmp(argv[0], "ls") == 0) {
            cmd_ls();
        } else if (strcmp(argv[0], "cat") == 0) {
            cmd_cat(argc, argv);
        } else if (strcmp(argv[0], "exit") == 0) {
            print("Exiting kernel-r shell...\n");
            syscall1(60, 0);
        } else {
            cmd_exec(argv[0]);
        }
    }
}
