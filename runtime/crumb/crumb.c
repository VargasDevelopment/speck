#if defined(CRUMB_PACED) && !defined(_POSIX_C_SOURCE)
#define _POSIX_C_SOURCE 200809L
#endif

#include "crumb.h"
#include "crumb_internal.h"

#include <stdint.h>
#include <stdio.h>

#if defined(CRUMB_DEVELOPMENT) || defined(CRUMB_COCOA)
#include <errno.h>
#include <limits.h>
#include <stdlib.h>
#endif

#ifdef CRUMB_PACED
#include <errno.h>
#include <time.h>
#endif

enum { CRUMB_PPM_FRAME_COUNT = 5 };

#ifdef CRUMB_PACED
enum { CRUMB_FRAME_NANOSECONDS = 16666667 };

static void advance_deadline(struct timespec *deadline) {
    deadline->tv_nsec += CRUMB_FRAME_NANOSECONDS;
    if (deadline->tv_nsec >= 1000000000L) {
        deadline->tv_sec += 1;
        deadline->tv_nsec -= 1000000000L;
    }
}

static int timespec_is_before(const struct timespec *left, const struct timespec *right) {
    return left->tv_sec < right->tv_sec ||
           (left->tv_sec == right->tv_sec && left->tv_nsec < right->tv_nsec);
}

static struct timespec timespec_difference(const struct timespec *later,
                                           const struct timespec *earlier) {
    struct timespec difference;

    difference.tv_sec = later->tv_sec - earlier->tv_sec;
    difference.tv_nsec = later->tv_nsec - earlier->tv_nsec;
    if (difference.tv_nsec < 0) {
        difference.tv_sec -= 1;
        difference.tv_nsec += 1000000000L;
    }
    return difference;
}

static int crumb_pacing_init(struct timespec *deadline) {
    return clock_gettime(CLOCK_MONOTONIC, deadline) == 0 ? 0 : 1;
}

static int crumb_wait_for_next_frame(struct timespec *deadline) {
    struct timespec now;

    advance_deadline(deadline);
    if (clock_gettime(CLOCK_MONOTONIC, &now) != 0) {
        return 1;
    }
    if (timespec_is_before(&now, deadline)) {
        struct timespec delay = timespec_difference(deadline, &now);

        while (nanosleep(&delay, &delay) != 0) {
            if (errno != EINTR) {
                return 1;
            }
            if (crumb_platform_should_stop()) {
                return 0;
            }
        }
    }
    if (clock_gettime(CLOCK_MONOTONIC, &now) != 0) {
        return 1;
    }
    {
        struct timespec missed_deadline = *deadline;
        advance_deadline(&missed_deadline);
        if (timespec_is_before(&missed_deadline, &now)) {
            *deadline = now;
        }
    }
    return 0;
}
#endif

static int crumb_frame_limit(void) {
#if defined(CRUMB_DEVELOPMENT) || defined(CRUMB_COCOA)
    const char *text = getenv("SPECK_FRAME_LIMIT");
    char *end = NULL;
    long value;

    if (text == NULL) {
        return 0;
    }
    errno = 0;
    value = strtol(text, &end, 10);
    if (errno != 0 || end == text || *end != '\0' || value <= 0 || value > INT_MAX) {
        return -1;
    }
    return (int)value;
#else
    return CRUMB_PPM_FRAME_COUNT;
#endif
}

int crumb_init(void) {
    crumb_input_reset();
    crumb_clear_rgb(0, 0, 0);
    return crumb_present_init();
}

float crumb_frame_delta(void) { return 1.0f / 60.0f; }

void crumb_print_i32(int value) { printf("%d\n", value); }

void crumb_debug_frame(int frame, float value) { printf("frame %d: %.3f\n", frame, (double)value); }

void crumb_shutdown(void) {
    crumb_input_release_all();
    crumb_present_shutdown();
}

int crumb_run(void) {
    uint64_t frames_completed = 0;
    const int frame_limit = crumb_frame_limit();
    const float dt = crumb_frame_delta();
#ifdef CRUMB_PACED
    struct timespec next_frame_deadline;
#endif

    if (frame_limit < 0) {
        fputs("CRuMB received an invalid frame limit\n", stderr);
        return 1;
    }
    if (crumb_platform_init() != 0) {
        fputs("CRuMB could not initialize platform lifecycle\n", stderr);
        return 1;
    }
    if (crumb_init() != 0) {
        fputs("CRuMB could not initialize its presenter\n", stderr);
        crumb_platform_shutdown();
        return 1;
    }
    spk_start();
#ifdef CRUMB_PACED
    if (crumb_pacing_init(&next_frame_deadline) != 0) {
        fputs("CRuMB could not initialize frame pacing\n", stderr);
        crumb_shutdown();
        crumb_platform_shutdown();
        return 1;
    }
#endif
    while ((frame_limit == 0 || frames_completed < (uint64_t)frame_limit) &&
           !crumb_platform_should_stop() && !crumb_quit_requested()) {
        int presentation;

        crumb_input_begin_frame();
        presentation = crumb_present_poll();
        if (presentation == CRUMB_PRESENT_ERROR) {
            fputs("CRuMB presenter event polling failed\n", stderr);
            crumb_shutdown();
            crumb_platform_shutdown();
            return 1;
        }
        if (presentation == CRUMB_PRESENT_STOP || crumb_platform_should_stop()) {
            break;
        }
        spk_update(dt);
        spk_draw();
        presentation = crumb_present();
        ++frames_completed;
        if (presentation == CRUMB_PRESENT_ERROR) {
            fputs("CRuMB presenter failed\n", stderr);
            crumb_shutdown();
            crumb_platform_shutdown();
            return 1;
        }
        if (presentation == CRUMB_PRESENT_STOP || crumb_platform_should_stop() ||
            crumb_quit_requested() ||
            (frame_limit != 0 && frames_completed >= (uint64_t)frame_limit)) {
            break;
        }
#ifdef CRUMB_PACED
        if (crumb_wait_for_next_frame(&next_frame_deadline) != 0) {
            fputs("CRuMB frame pacing failed\n", stderr);
            crumb_shutdown();
            crumb_platform_shutdown();
            return 1;
        }
#endif
    }
    crumb_shutdown();
    crumb_platform_shutdown();
    return 0;
}
