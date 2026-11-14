// Test-only observer, injected only into a synthetic parity-preview process.
// MTLDrawable.presentedTime is an actual onscreen timestamp, not a draw request.
// https://developer.apple.com/documentation/metal/mtldrawable/presentedtime
#import <Foundation/Foundation.h>
#import <AppKit/AppKit.h>
#import <QuartzCore/QuartzCore.h>
#import <Metal/Metal.h>
#import <objc/runtime.h>
#include <fcntl.h>
#include <pthread.h>
#include <stdatomic.h>
#include <unistd.h>

static int output = -1;
static pthread_mutex_t outputLock = PTHREAD_MUTEX_INITIALIZER;
static _Atomic unsigned sequence;
static CFTimeInterval started;
static void (*originalPresentDrawable)(id, SEL, id<MTLDrawable>);

static void record(const char *line, int length) {
    pthread_mutex_lock(&outputLock);
    ssize_t written = write(output, line, (size_t)length);
    // A truncated trace must fail validation rather than silently lose events.
    if (written != length) _exit(74);
    pthread_mutex_unlock(&outputLock);
}

static void observedPresentDrawable(id buffer, SEL selector, id<CAMetalDrawable> drawable) {
    CFTimeInterval submitted = CACurrentMediaTime();
    unsigned index = atomic_fetch_add(&sequence, 1);
    char line[320];
    int length = snprintf(line, sizeof(line),
        "{\"event\":\"submit\",\"index\":%u,\"atMs\":%.6f,\"pixels\":[%lu,%lu]}\n",
        index, (submitted-started)*1000, (unsigned long)drawable.texture.width,
        (unsigned long)drawable.texture.height);
    record(line, length);
    [drawable addPresentedHandler:^(id<MTLDrawable> presented) {
        CFTimeInterval time = presented.presentedTime;
        char line[256];
        int length = snprintf(line, sizeof(line),
            "{\"event\":\"present\",\"index\":%u,\"atMs\":%.6f,\"callbackMs\":%.6f,\"valid\":%s}\n",
            index, time ? (time-started)*1000 : 0,
            (CACurrentMediaTime()-started)*1000, time > 0 ? "true" : "false");
        record(line, length);
    }];
    originalPresentDrawable(buffer, selector, drawable);
}

__attribute__((constructor)) static void installObserver(void) {
    @autoreleasepool {
        NSArray<NSString *> *arguments = NSProcessInfo.processInfo.arguments;
        const char *path = getenv("AIPASS_PRESENTATION_TRACE");
        if (!path || ![arguments containsObject:@"--parity-preview"]) _exit(64);
        for (NSString *argument in arguments) {
            for (NSString *prefix in @[@"--vault=", @"--update-config=", @"--capture=",
                                      @"--backdrop-at=", @"--motion-at="]) {
                if ([argument hasPrefix:prefix]) _exit(64);
            }
        }
        output = open(path, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0600);
        if (output < 0) _exit(73);
        started = CACurrentMediaTime();
        char header[240];
        int length = snprintf(header, sizeof(header),
            "{\"event\":\"start\",\"pid\":%d,\"clock\":\"MTLDrawable.presentedTime\",\"wallUnixMs\":%.3f,\"observerIncluded\":true}\n",
            getpid(), NSDate.date.timeIntervalSince1970*1000);
        record(header, length);
        for (NSString *name in @[@"NSWindowDidBecomeKeyNotification", @"NSWindowDidResignKeyNotification",
                                 @"NSApplicationDidBecomeActiveNotification", @"NSApplicationDidResignActiveNotification"]) {
            [NSNotificationCenter.defaultCenter addObserverForName:name object:nil queue:nil
                usingBlock:^(NSNotification *notification) {
                    char line[256];
                    int length = snprintf(line, sizeof(line),
                        "{\"event\":\"window-state\",\"atMs\":%.6f,\"name\":\"%s\"}\n",
                        (CACurrentMediaTime()-started)*1000, notification.name.UTF8String);
                    record(line, length);
                }];
        }
        [NSWorkspace.sharedWorkspace.notificationCenter
            addObserverForName:NSWorkspaceDidActivateApplicationNotification object:nil queue:nil
            usingBlock:^(NSNotification *notification) {
                NSRunningApplication *app = notification.userInfo[NSWorkspaceApplicationKey];
                NSDictionary *event = @{@"event": @"foreground-app",
                    @"atMs": @((CACurrentMediaTime()-started)*1000),
                    @"pid": @(app.processIdentifier), @"bundleId": app.bundleIdentifier ?: @""};
                NSData *data = [NSJSONSerialization dataWithJSONObject:event options:0 error:nil];
                NSMutableData *line = [data mutableCopy];
                [line appendBytes:"\n" length:1];
                record(line.bytes, (int)line.length);
            }];
        // Modern GPUI receives drawables from CAMetalDisplayLink; nextDrawable
        // alone observes only startup. Observe actual command-buffer submits.
        id<MTLDevice> device = MTLCreateSystemDefaultDevice();
        id<MTLCommandQueue> queue = [device newCommandQueue];
        id<MTLCommandBuffer> buffer = [queue commandBuffer];
        Method method = class_getInstanceMethod([buffer class], @selector(presentDrawable:));
        if (!method) _exit(69);
        originalPresentDrawable = (void *)method_setImplementation(method, (IMP)observedPresentDrawable);
    }
}
