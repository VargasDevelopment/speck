#include "crumb.h"
#include "crumb_internal.h"

#import <AppKit/AppKit.h>
#import <CoreGraphics/CoreGraphics.h>

#include <math.h>

static NSWindow *crumb_window = nil;
static NSView *crumb_view = nil;
static NSObject<NSWindowDelegate> *crumb_window_delegate = nil;
static int crumb_window_closed = 0;

@interface CrumbFramebufferView : NSView
@end

@implementation CrumbFramebufferView

- (BOOL)isOpaque {
    return YES;
}

- (void)drawRect:(NSRect)dirty_rect {
    const unsigned char *pixels = crumb_framebuffer_pixels();
    const NSRect bounds = [self bounds];
    const NSRect backing_bounds = [self convertRectToBacking:bounds];
    const CGFloat width_scale = NSWidth(backing_bounds) / CRUMB_FRAMEBUFFER_WIDTH;
    const CGFloat height_scale = NSHeight(backing_bounds) / CRUMB_FRAMEBUFFER_HEIGHT;
    CGFloat pixel_scale = fmin(width_scale, height_scale);
    CGFloat target_width;
    CGFloat target_height;
    NSRect backing_target;
    NSRect target;
    CGContextRef context;
    CGColorSpaceRef color_space;
    CGDataProviderRef provider;
    CGImageRef image;

    (void)dirty_rect;
    [[NSColor blackColor] setFill];
    NSRectFill(bounds);
    if (pixel_scale >= 1.0) {
        pixel_scale = floor(pixel_scale);
    }
    target_width = CRUMB_FRAMEBUFFER_WIDTH * pixel_scale;
    target_height = CRUMB_FRAMEBUFFER_HEIGHT * pixel_scale;
    backing_target = NSMakeRect(floor((NSWidth(backing_bounds) - target_width) * 0.5),
                                floor((NSHeight(backing_bounds) - target_height) * 0.5),
                                target_width, target_height);
    target = [self convertRectFromBacking:backing_target];

    context = [[NSGraphicsContext currentContext] CGContext];
    color_space = CGColorSpaceCreateDeviceRGB();
    provider = CGDataProviderCreateWithData(NULL, pixels, CRUMB_FRAMEBUFFER_BYTES, NULL);
    if (context == NULL || color_space == NULL || provider == NULL) {
        if (provider != NULL) {
            CGDataProviderRelease(provider);
        }
        if (color_space != NULL) {
            CGColorSpaceRelease(color_space);
        }
        return;
    }
    image = CGImageCreate(CRUMB_FRAMEBUFFER_WIDTH, CRUMB_FRAMEBUFFER_HEIGHT, 8, 24,
                          CRUMB_FRAMEBUFFER_STRIDE, color_space, (CGBitmapInfo)kCGImageAlphaNone,
                          provider, NULL, false, kCGRenderingIntentDefault);
    if (image != NULL) {
        CGContextSetInterpolationQuality(context, kCGInterpolationNone);
        CGContextSetShouldAntialias(context, false);
        CGContextDrawImage(context, NSRectToCGRect(target), image);
        CGImageRelease(image);
    }
    CGDataProviderRelease(provider);
    CGColorSpaceRelease(color_space);
}

@end

@interface CrumbWindowDelegate : NSObject <NSWindowDelegate>
@end

@implementation CrumbWindowDelegate

- (void)windowWillClose:(NSNotification *)notification {
    (void)notification;
    crumb_window_closed = 1;
}

@end

int crumb_present_init(void) {
    @autoreleasepool {
        const NSWindowStyleMask style = NSWindowStyleMaskTitled | NSWindowStyleMaskClosable |
                                        NSWindowStyleMaskMiniaturizable |
                                        NSWindowStyleMaskResizable;
        const NSRect content_rect = NSMakeRect(0.0, 0.0, 960.0, 540.0);

        [NSApplication sharedApplication];
        if (![NSApp setActivationPolicy:NSApplicationActivationPolicyRegular]) {
            return CRUMB_PRESENT_ERROR;
        }
        [NSApp finishLaunching];

        crumb_window = [[NSWindow alloc] initWithContentRect:content_rect
                                                   styleMask:style
                                                     backing:NSBackingStoreBuffered
                                                       defer:NO];
        crumb_view = [[CrumbFramebufferView alloc] initWithFrame:content_rect];
        crumb_window_delegate = [[CrumbWindowDelegate alloc] init];
        if (crumb_window == nil || crumb_view == nil || crumb_window_delegate == nil) {
            crumb_present_shutdown();
            return CRUMB_PRESENT_ERROR;
        }
        [crumb_window setReleasedWhenClosed:NO];
        [crumb_window setDelegate:crumb_window_delegate];
        [crumb_window setContentView:crumb_view];
        [crumb_window
            setContentMinSize:NSMakeSize(CRUMB_FRAMEBUFFER_WIDTH, CRUMB_FRAMEBUFFER_HEIGHT)];
        [crumb_window setTitle:@"Speck"];
        [crumb_window center];
        [crumb_window makeKeyAndOrderFront:nil];
        [NSApp activate];
        crumb_window_closed = 0;
    }
    return CRUMB_PRESENT_CONTINUE;
}

int crumb_present(void) {
    @autoreleasepool {
        NSEvent *event;

        while ((event = [NSApp nextEventMatchingMask:NSEventMaskAny
                                           untilDate:[NSDate distantPast]
                                              inMode:NSDefaultRunLoopMode
                                             dequeue:YES]) != nil) {
            [NSApp sendEvent:event];
        }
        [NSApp updateWindows];
        if (crumb_window_closed) {
            return CRUMB_PRESENT_STOP;
        }
        [crumb_view setNeedsDisplay:YES];
        [crumb_view displayIfNeeded];
    }
    return CRUMB_PRESENT_CONTINUE;
}

void crumb_present_shutdown(void) {
    @autoreleasepool {
        if (crumb_window != nil) {
            [crumb_window setDelegate:nil];
            [crumb_window orderOut:nil];
            [crumb_window close];
        }
        [crumb_view release];
        [crumb_window release];
        [crumb_window_delegate release];
        crumb_view = nil;
        crumb_window = nil;
        crumb_window_delegate = nil;
        crumb_window_closed = 1;
    }
}
