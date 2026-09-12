#include "crumb.h"
#include "crumb_internal.h"

#import <AppKit/AppKit.h>
#include <IOKit/hidsystem/IOLLEvent.h>

#include <assert.h>

#import "../runtime/crumb/present_cocoa.m"

@interface CrumbTestKeyEvent : NSEvent {
    unsigned short test_key_code;
    BOOL test_repeat;
    NSEventModifierFlags test_flags;
}

- (instancetype)initWithKeyCode:(unsigned short)key_code repeat:(BOOL)repeat flags:(NSEventModifierFlags)flags;

@end

@implementation CrumbTestKeyEvent

- (instancetype)initWithKeyCode:(unsigned short)key_code repeat:(BOOL)repeat flags:(NSEventModifierFlags)flags {
    self = [super init];
    if (self != nil) {
        test_key_code = key_code;
        test_repeat = repeat;
        test_flags = flags;
    }
    return self;
}

- (unsigned short)keyCode {
    return test_key_code;
}

- (NSEventModifierFlags)modifierFlags {
    return test_flags;
}

- (BOOL)isARepeat {
    return test_repeat;
}

@end

static NSEvent *key_event(unsigned short key_code, BOOL repeat) {
    return [[[CrumbTestKeyEvent alloc] initWithKeyCode:key_code repeat:repeat flags:0] autorelease];
}

static NSEvent *modifier_event(unsigned short code, NSEventModifierFlags flags) {
    return [[[CrumbTestKeyEvent alloc] initWithKeyCode:code repeat:NO flags:flags] autorelease];
}

int main(void) {
    @autoreleasepool {
        CrumbFramebufferView *view = [[[CrumbFramebufferView alloc] init] autorelease];
        CrumbWindowDelegate *delegate = [[[CrumbWindowDelegate alloc] init] autorelease];

        // Independent SDK examples from each physical-key family. A shared
        // catalog cannot establish that its own native mapping data is right.
        const struct { unsigned short code; int key; } examples[] = {
            {12, CRUMB_KEY_Q}, {6, CRUMB_KEY_Z}, {18, CRUMB_KEY_1}, {29, CRUMB_KEY_0},
            {41, CRUMB_KEY_SEMICOLON}, {44, CRUMB_KEY_SLASH}, {48, CRUMB_KEY_TAB},
            {51, CRUMB_KEY_BACKSPACE}, {117, CRUMB_KEY_DELETE}, {115, CRUMB_KEY_HOME},
            {116, CRUMB_KEY_PAGE_UP}, {122, CRUMB_KEY_F1}, {111, CRUMB_KEY_F12},
            {82, CRUMB_KEY_NUMPAD_0}, {76, CRUMB_KEY_NUMPAD_ENTER}
        };
        for (size_t i = 0; i < sizeof examples / sizeof examples[0]; ++i) {
            crumb_input_reset();
            [view keyDown:key_event(examples[i].code, NO)];
            assert(crumb_key_down(examples[i].key));
            assert(crumb_key_pressed(examples[i].key));
            [view keyUp:key_event(examples[i].code, NO)];
            assert(crumb_key_released(examples[i].key));
        }
        // Both sides of a modifier can be held independently. Releasing one
        // must not clear the other; focus loss must clear the remaining side.
        const struct {
            unsigned short left_code, right_code;
            int left_key, right_key;
            NSEventModifierFlags left_mask, right_mask, aggregate;
        } modifiers[] = {
            {56, 60, CRUMB_KEY_SHIFT_LEFT, CRUMB_KEY_SHIFT_RIGHT,
                NX_DEVICELSHIFTKEYMASK, NX_DEVICERSHIFTKEYMASK, NSEventModifierFlagShift},
            {59, 62, CRUMB_KEY_CONTROL_LEFT, CRUMB_KEY_CONTROL_RIGHT,
                NX_DEVICELCTLKEYMASK, NX_DEVICERCTLKEYMASK, NSEventModifierFlagControl},
            {58, 61, CRUMB_KEY_ALT_LEFT, CRUMB_KEY_ALT_RIGHT,
                NX_DEVICELALTKEYMASK, NX_DEVICERALTKEYMASK, NSEventModifierFlagOption},
            {55, 54, CRUMB_KEY_META_LEFT, CRUMB_KEY_META_RIGHT,
                NX_DEVICELCMDKEYMASK, NX_DEVICERCMDKEYMASK, NSEventModifierFlagCommand}
        };
        for (size_t i = 0; i < sizeof modifiers / sizeof modifiers[0]; ++i) {
            crumb_input_reset();
            [view flagsChanged:modifier_event(modifiers[i].left_code,
                modifiers[i].left_mask | modifiers[i].aggregate)];
            [view flagsChanged:modifier_event(modifiers[i].right_code,
                modifiers[i].left_mask | modifiers[i].right_mask | modifiers[i].aggregate)];
            assert(crumb_key_pressed(modifiers[i].left_key));
            assert(crumb_key_pressed(modifiers[i].right_key));
            crumb_input_begin_frame();
            [view flagsChanged:modifier_event(modifiers[i].left_code,
                modifiers[i].right_mask | modifiers[i].aggregate)];
            assert(!crumb_key_down(modifiers[i].left_key));
            assert(crumb_key_released(modifiers[i].left_key));
            assert(crumb_key_down(modifiers[i].right_key));
            [delegate windowDidResignKey:[NSNotification notificationWithName:@"Test" object:nil]];
            assert(!crumb_key_down(modifiers[i].right_key));
            assert(crumb_key_released(modifiers[i].right_key));
        }

        assert([view acceptsFirstResponder]);
        crumb_input_reset();
        [view keyDown:key_event(0, NO)];
        assert(crumb_key_down(CRUMB_KEY_A));
        assert(crumb_key_pressed(CRUMB_KEY_A));

        crumb_input_begin_frame();
        [view keyDown:key_event(0, YES)];
        assert(crumb_key_down(CRUMB_KEY_A));
        assert(!crumb_key_pressed(CRUMB_KEY_A));
        [view keyUp:key_event(0, NO)];
        assert(!crumb_key_down(CRUMB_KEY_A));
        assert(crumb_key_released(CRUMB_KEY_A));

        // Physical macOS F is code 3; keep this independent of the mapping enum.
        crumb_input_begin_frame();
        [view keyDown:key_event(3, NO)];
        assert(crumb_key_down(CRUMB_KEY_F));
        assert(crumb_key_pressed(CRUMB_KEY_F));
        assert(!crumb_key_down(CRUMB_KEY_D));
        crumb_input_begin_frame();
        [view keyDown:key_event(3, YES)];
        assert(crumb_key_down(CRUMB_KEY_F));
        assert(!crumb_key_pressed(CRUMB_KEY_F));
        [view keyUp:key_event(3, NO)];
        assert(!crumb_key_down(CRUMB_KEY_F));
        assert(crumb_key_released(CRUMB_KEY_F));

        crumb_input_begin_frame();
        [view keyDown:key_event(3, NO)];
        [view keyDown:key_event(53, NO)];
        assert(crumb_key_down(CRUMB_KEY_ESCAPE));
        assert(crumb_key_pressed(CRUMB_KEY_ESCAPE));

        [view keyDown:key_event(123, NO)];
        [delegate windowDidResignKey:[NSNotification notificationWithName:@"Test" object:nil]];
        assert(!crumb_key_down(CRUMB_KEY_F));
        assert(crumb_key_released(CRUMB_KEY_F));
        assert(!crumb_key_down(CRUMB_KEY_ESCAPE));
        assert(!crumb_key_down(CRUMB_KEY_LEFT));
        assert(crumb_key_released(CRUMB_KEY_ESCAPE));
        assert(crumb_key_released(CRUMB_KEY_LEFT));
    }
    return 0;
}
