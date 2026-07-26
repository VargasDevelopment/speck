#include "crumb.h"
#include "crumb_internal.h"

#include <stdio.h>

#define CRUMB_STRINGIFY_INNER(value) #value
#define CRUMB_STRINGIFY(value) CRUMB_STRINGIFY_INNER(value)

int crumb_present_init(void) { return CRUMB_PRESENT_CONTINUE; }

int crumb_present(void) {
    static const char header[] = "P6\n" CRUMB_STRINGIFY(
        CRUMB_FRAMEBUFFER_WIDTH) " " CRUMB_STRINGIFY(CRUMB_FRAMEBUFFER_HEIGHT) "\n255\n";
    FILE *output = fopen("build/frame.ppm", "wb");
    int failed = 0;

    if (output == NULL) {
        return CRUMB_PRESENT_ERROR;
    }
    if (fwrite(header, 1, sizeof(header) - 1, output) != sizeof(header) - 1) {
        failed = 1;
    }
    if (!failed && fwrite(crumb_framebuffer_pixels(), 1, CRUMB_FRAMEBUFFER_BYTES, output) !=
                       CRUMB_FRAMEBUFFER_BYTES) {
        failed = 1;
    }
    if (fclose(output) != 0) {
        failed = 1;
    }
    return failed ? CRUMB_PRESENT_ERROR : CRUMB_PRESENT_CONTINUE;
}

void crumb_present_shutdown(void) {}

#undef CRUMB_STRINGIFY
#undef CRUMB_STRINGIFY_INNER
