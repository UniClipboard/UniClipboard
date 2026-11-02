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
#import <CommonCrypto/CommonDigest.h>

static NSString *private_name(void) {
    const char *name = getenv("UC_E2E_PRIVATE_PASTEBOARD");
    if (!name || strncmp(name, "org.uniclipboard.e2e.", 21) != 0) abort();
    return [NSString stringWithUTF8String:name];
}

#ifdef PRIVATE_PASTEBOARD_TOOL
static NSString *digest(NSData *data) {
    unsigned char bytes[CC_SHA256_DIGEST_LENGTH];
    CC_SHA256(data.bytes, (CC_LONG)data.length, bytes);
    NSMutableString *result = [NSMutableString string];
    for (NSUInteger i = 0; i < sizeof(bytes); i++) [result appendFormat:@"%02x", bytes[i]];
    return result;
}

static NSDictionary *inspect(NSPasteboard *board) {
    NSInteger before = board.changeCount;
    NSMutableDictionary *result = [@{@"changeCount": @(before)} mutableCopy];
    NSString *text = [board stringForType:NSPasteboardTypeString];
    if (text) result[@"textSha256"] = digest([text dataUsingEncoding:NSUTF8StringEncoding]);
    NSData *image = [board dataForType:NSPasteboardTypePNG] ?: [board dataForType:NSPasteboardTypeTIFF];
    if (image) {
        NSBitmapImageRep *bitmap = [NSBitmapImageRep imageRepWithData:image];
        if (!bitmap) return nil;
        NSMutableData *rgb = [NSMutableData data];
        for (NSInteger y = 0; y < bitmap.pixelsHigh; y++) {
            for (NSInteger x = 0; x < bitmap.pixelsWide; x++) {
                NSColor *color = [[bitmap colorAtX:x y:y] colorUsingColorSpace:NSColorSpace.sRGBColorSpace];
                if (!color) return nil;
                unsigned char pixel[3] = {lround(color.redComponent * 255), lround(color.greenComponent * 255), lround(color.blueComponent * 255)};
                [rgb appendBytes:pixel length:3];
            }
        }
        result[@"image"] = @{@"width": @(bitmap.pixelsWide), @"height": @(bitmap.pixelsHigh), @"pixelsSha256": digest(rgb)};
    }
    NSArray *urls = [board readObjectsForClasses:@[NSURL.class] options:@{NSPasteboardURLReadingFileURLsOnlyKey: @YES}];
    NSMutableArray *files = [NSMutableArray array];
    for (NSURL *url in urls) {
        NSData *data = [NSData dataWithContentsOfURL:url];
        if (!data) return nil;
        [files addObject:@{@"size": @(data.length), @"sha256": digest(data)}];
    }
    result[@"files"] = files;
    if (before != board.changeCount) return nil;
    return result;
}

int main(int argc, const char **argv) {
    @autoreleasepool {
        NSPasteboard *board = [NSPasteboard pasteboardWithName:private_name()];
        if (argc == 2 && strcmp(argv[1], "inspect") == 0) {
            NSDictionary *summary = inspect(board);
            if (!summary) return 3;
            NSData *json = [NSJSONSerialization dataWithJSONObject:summary options:0 error:nil];
            if (!json || fwrite(json.bytes, 1, json.length, stdout) != json.length) return 3;
            return 0;
        }
        if (argc == 2 && strcmp(argv[1], "release") == 0) {
            [board releaseGlobally];
            return 0;
        }
        if (argc == 2 && strcmp(argv[1], "clear") == 0) {
            [board clearContents];
            return 0;
        }
        NSArray *objects = nil;
        if (argc == 2 && strcmp(argv[1], "recopy") == 0) {
            NSMutableArray *copies = [NSMutableArray array];
            for (NSPasteboardItem *item in board.pasteboardItems) {
                NSPasteboardItem *copy = [[NSPasteboardItem alloc] init];
                for (NSString *type in item.types) {
                    NSData *data = [item dataForType:type];
                    if (!data || ![copy setData:data forType:type]) return 3;
                }
                [copies addObject:copy];
            }
            if (copies.count == 0) return 3;
            objects = copies;
        } else if (argc >= 3 && (strcmp(argv[1], "file") == 0 || strcmp(argv[1], "files") == 0)) {
            NSMutableArray *urls = [NSMutableArray array];
            for (int i = 2; i < argc; i++) {
                NSURL *url = [NSURL fileURLWithPath:[NSString stringWithUTF8String:argv[i]]];
                if (![url checkResourceIsReachableAndReturnError:nil]) return 3;
                [urls addObject:url];
            }
            objects = urls;
        } else if (argc == 3 && strcmp(argv[1], "image") == 0) {
            NSData *data = [NSData dataWithContentsOfFile:[NSString stringWithUTF8String:argv[2]]];
            if (!data || ![NSBitmapImageRep imageRepWithData:data]) return 3;
            NSPasteboardItem *item = [[NSPasteboardItem alloc] init];
            if (![item setData:data forType:NSPasteboardTypePNG]) return 3;
            objects = @[item];
        } else if (argc == 3 && strcmp(argv[1], "text") == 0) {
            objects = @[[NSString stringWithUTF8String:argv[2]]];
        } else return 2;
        [board clearContents];
        if (![board writeObjects:objects]) return 3;
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
