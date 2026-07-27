#include "crumb.h"
#include "crumb_internal.h"

#import <AppKit/AppKit.h>

#include <assert.h>

#import "../runtime/crumb/present_cocoa.m"

@interface CrumbTestKeyEvent : NSEvent {
    unsigned short test_key_code;
    BOOL test_repeat;
}

- (instancetype)initWithKeyCode:(unsigned short)key_code repeat:(BOOL)repeat;

@end

@implementation CrumbTestKeyEvent

- (instancetype)initWithKeyCode:(unsigned short)key_code repeat:(BOOL)repeat {
    self = [super init];
    if (self != nil) {
        test_key_code = key_code;
        test_repeat = repeat;
    }
    return self;
}

- (unsigned short)keyCode {
    return test_key_code;
}

- (BOOL)isARepeat {
    return test_repeat;
}

@end

static NSEvent *key_event(unsigned short key_code, BOOL repeat) {
    return [[[CrumbTestKeyEvent alloc] initWithKeyCode:key_code repeat:repeat] autorelease];
}

int main(void) {
    @autoreleasepool {
        CrumbFramebufferView *view = [[[CrumbFramebufferView alloc] init] autorelease];
        CrumbWindowDelegate *delegate = [[[CrumbWindowDelegate alloc] init] autorelease];

        assert([view acceptsFirstResponder]);
        crumb_input_reset();
        [view keyDown:key_event(CRUMB_MAC_KEY_A, NO)];
        assert(crumb_key_down(CRUMB_KEY_A));
        assert(crumb_key_pressed(CRUMB_KEY_A));

        crumb_input_begin_frame();
        [view keyDown:key_event(CRUMB_MAC_KEY_A, YES)];
        assert(crumb_key_down(CRUMB_KEY_A));
        assert(!crumb_key_pressed(CRUMB_KEY_A));
        [view keyUp:key_event(CRUMB_MAC_KEY_A, NO)];
        assert(!crumb_key_down(CRUMB_KEY_A));
        assert(crumb_key_released(CRUMB_KEY_A));

        crumb_input_begin_frame();
        [view keyDown:key_event(CRUMB_MAC_KEY_ESCAPE, NO)];
        assert(crumb_key_down(CRUMB_KEY_ESCAPE));
        assert(crumb_key_pressed(CRUMB_KEY_ESCAPE));

        [view keyDown:key_event(CRUMB_MAC_KEY_LEFT, NO)];
        [delegate windowDidResignKey:[NSNotification notificationWithName:@"Test" object:nil]];
        assert(!crumb_key_down(CRUMB_KEY_ESCAPE));
        assert(!crumb_key_down(CRUMB_KEY_LEFT));
        assert(crumb_key_released(CRUMB_KEY_ESCAPE));
        assert(crumb_key_released(CRUMB_KEY_LEFT));
    }
    return 0;
}
