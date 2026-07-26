#include "crumb.h"

#include <assert.h>
#include <limits.h>

static void assert_pixel(int x, int y, int red, int green, int blue) {
    const unsigned char *pixels = crumb_framebuffer_pixels();
    const int offset = y * CRUMB_FRAMEBUFFER_STRIDE + x * CRUMB_FRAMEBUFFER_CHANNELS;

    assert(pixels[offset] == red);
    assert(pixels[offset + 1] == green);
    assert(pixels[offset + 2] == blue);
}

int main(void) {
    crumb_clear_rgb(-10, 128, 300);
    assert_pixel(0, 0, 0, 128, 255);
    assert_pixel(CRUMB_FRAMEBUFFER_WIDTH - 1, CRUMB_FRAMEBUFFER_HEIGHT - 1, 0, 128, 255);

    crumb_fill_rect(-2, -1, 4, 3, 300, -4, 42);
    assert_pixel(0, 0, 255, 0, 42);
    assert_pixel(1, 1, 255, 0, 42);
    assert_pixel(2, 0, 0, 128, 255);
    assert_pixel(0, 2, 0, 128, 255);

    crumb_fill_rect(0, 0, -1, 10, 1, 2, 3);
    crumb_fill_rect(INT_MAX, INT_MAX, INT_MAX, INT_MAX, 1, 2, 3);
    crumb_fill_rect(INT_MIN, INT_MIN, INT_MAX, INT_MAX, 1, 2, 3);
    assert_pixel(2, 0, 0, 128, 255);
    assert_pixel(CRUMB_FRAMEBUFFER_WIDTH - 1, CRUMB_FRAMEBUFFER_HEIGHT - 1, 0, 128, 255);
    return 0;
}
