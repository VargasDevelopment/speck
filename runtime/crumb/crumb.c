#include "crumb.h"

#include <stdio.h>

enum { CRUMB_FRAME_COUNT = 5 };

int crumb_init(void) { return 0; }

float crumb_frame_delta(void) { return 1.0f / 60.0f; }

void crumb_print_i32(int value) { printf("%d\n", value); }

void crumb_debug_frame(int frame, float value) { printf("frame %d: %.3f\n", frame, (double)value); }

void crumb_shutdown(void) {}

int crumb_run(void) {
    int frame;
    const float dt = crumb_frame_delta();

    if (crumb_init() != 0) {
        return 1;
    }
    spk_start();
    for (frame = 0; frame < CRUMB_FRAME_COUNT; ++frame) {
        spk_update(dt);
        spk_draw();
    }
    crumb_shutdown();
    return 0;
}

int main(void) { return crumb_run(); }
