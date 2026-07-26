#if defined(CRUMB_DEVELOPMENT) && !defined(_POSIX_C_SOURCE)
#define _POSIX_C_SOURCE 199309L
#endif

#include "crumb.h"
#include "crumb_internal.h"

#include <stdio.h>

#ifdef CRUMB_DEVELOPMENT
#include <errno.h>
#include <limits.h>
#include <stdlib.h>
#include <time.h>
#endif

enum { CRUMB_FRAME_COUNT = 5 };

static int crumb_frame_limit(void) {
#ifdef CRUMB_DEVELOPMENT
    const char *text = getenv("SPECK_FRAME_LIMIT");
    char *end = NULL;
    long value;

    if (text == NULL) {
        return 1800;
    }
    errno = 0;
    value = strtol(text, &end, 10);
    if (errno != 0 || end == text || *end != '\0' || value <= 0 || value > INT_MAX) {
        return -1;
    }
    return (int)value;
#else
    return CRUMB_FRAME_COUNT;
#endif
}

static void crumb_wait_for_next_frame(void) {
#ifdef CRUMB_DEVELOPMENT
    struct timespec delay = {0, 16666667};

    while (nanosleep(&delay, &delay) != 0 && errno == EINTR) {
    }
#endif
}

int crumb_init(void) {
    crumb_clear_rgb(0, 0, 0);
    return crumb_present_init();
}

float crumb_frame_delta(void) { return 1.0f / 60.0f; }

void crumb_print_i32(int value) { printf("%d\n", value); }

void crumb_debug_frame(int frame, float value) { printf("frame %d: %.3f\n", frame, (double)value); }

void crumb_shutdown(void) { crumb_present_shutdown(); }

int crumb_run(void) {
    int frame;
    const int frame_limit = crumb_frame_limit();
    const float dt = crumb_frame_delta();

    if (frame_limit <= 0) {
        fputs("CRuMB received an invalid development frame limit\n", stderr);
        return 1;
    }
    if (crumb_init() != 0) {
        fputs("CRuMB could not initialize its presenter\n", stderr);
        return 1;
    }
    spk_start();
    for (frame = 0; frame < frame_limit; ++frame) {
        spk_update(dt);
        spk_draw();
        if (crumb_present() != 0) {
            fputs("CRuMB presenter failed\n", stderr);
            crumb_shutdown();
            return 1;
        }
        crumb_wait_for_next_frame();
    }
    crumb_shutdown();
    return 0;
}
