// Isolated E2E only: redirect generalPasteboard to a unique named board.
#import <AppKit/AppKit.h>
#import <objc/runtime.h>
#include <stdlib.h>
#include <dlfcn.h>
#include <fcntl.h>
#include <errno.h>
#include <stdarg.h>
#include <unistd.h>
#include <limits.h>
#include <sys/syscall.h>

static NSString *private_name(void) {
    const char *name = getenv("UC_E2E_PRIVATE_PASTEBOARD");
    if (!name || strncmp(name, "org.uniclipboard.e2e.", 21) != 0) abort();
    return [NSString stringWithUTF8String:name];
}

#ifdef PRIVATE_PASTEBOARD_TOOL
int main(int argc, const char **argv) {
    @autoreleasepool {
        NSPasteboard *board = [NSPasteboard pasteboardWithName:private_name()];
        [board clearContents];
        if (argc == 3 && strcmp(argv[1], "file") == 0) {
            NSURL *url = [NSURL fileURLWithPath:[NSString stringWithUTF8String:argv[2]]];
            [board writeObjects:@[url]];
        } else if (argc == 3 && strcmp(argv[1], "text") == 0) {
            [board setString:[NSString stringWithUTF8String:argv[2]] forType:NSPasteboardTypeString];
        } else if (argc == 2 && strcmp(argv[1], "release") == 0) {
            [board releaseGlobally];
        } else return 2;
    }
    return 0;
}
#else
// Inject faults only for the source explicitly selected by this test.
static int isolated_open(const char *path, int flags, ...) {
    mode_t mode = 0;
    if (flags & O_CREAT) { va_list args; va_start(args, flags); mode = va_arg(args, int); va_end(args); }
    const char *source = getenv("UC_E2E_SOURCE_FILE");
    const char *fault = getenv("UC_E2E_FILE_FAULT");
    if (source && fault && strcmp(path, source) == 0 && strcmp(fault, "missing") == 0) {
        errno = ENOENT; return -1;
    }
    return (int)syscall(SYS_open, path, flags, mode);
}
static ssize_t isolated_read(int descriptor, void *buffer, size_t count) {
    const char *source = getenv("UC_E2E_SOURCE_FILE");
    const char *fault = getenv("UC_E2E_FILE_FAULT");
    char path[PATH_MAX];
    if (source && fault && strcmp(fault, "middle") == 0 &&
        fcntl(descriptor, F_GETPATH, path) == 0 && strcmp(path, source) == 0 &&
        lseek(descriptor, 0, SEEK_CUR) >= 65536) {
        errno = EIO; return -1;
    }
    return (ssize_t)syscall(SYS_read, descriptor, buffer, count);
}
#define INTERPOSE(replacement, original) \
__attribute__((used)) static struct { const void *new_function; const void *old_function; } \
interpose_##original __attribute__((section("__DATA,__interpose"))) = { (const void *)&replacement, (const void *)&original };
INTERPOSE(isolated_open, open)
INTERPOSE(isolated_read, read)

static id isolated_general(id self, SEL command) {
    return [NSPasteboard pasteboardWithName:private_name()];
}
__attribute__((constructor)) static void install_private_pasteboard(void) {
    Method method = class_getClassMethod([NSPasteboard class], @selector(generalPasteboard));
    method_setImplementation(method, (IMP)isolated_general);
}
#endif
