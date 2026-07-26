#include "crumb.h"

#include <stdint.h>

static unsigned char pixels[CRUMB_FRAMEBUFFER_BYTES];

static unsigned char clamp_color(int component) {
    if (component < 0) {
        return 0;
    }
    if (component > 255) {
        return 255;
    }
    return (unsigned char)component;
}

static void write_pixel(int x, int y, unsigned char red, unsigned char green, unsigned char blue) {
    const int offset = y * CRUMB_FRAMEBUFFER_STRIDE + x * CRUMB_FRAMEBUFFER_CHANNELS;
    pixels[offset] = red;
    pixels[offset + 1] = green;
    pixels[offset + 2] = blue;
}

void crumb_clear_rgb(int red, int green, int blue) {
    int x;
    int y;
    const unsigned char clamped_red = clamp_color(red);
    const unsigned char clamped_green = clamp_color(green);
    const unsigned char clamped_blue = clamp_color(blue);

    for (y = 0; y < CRUMB_FRAMEBUFFER_HEIGHT; ++y) {
        for (x = 0; x < CRUMB_FRAMEBUFFER_WIDTH; ++x) {
            write_pixel(x, y, clamped_red, clamped_green, clamped_blue);
        }
    }
}

void crumb_fill_rect(int x, int y, int width, int height, int red, int green, int blue) {
    int draw_x;
    int draw_y;
    int64_t left;
    int64_t top;
    int64_t right;
    int64_t bottom;
    unsigned char clamped_red;
    unsigned char clamped_green;
    unsigned char clamped_blue;

    if (width <= 0 || height <= 0) {
        return;
    }

    left = x;
    top = y;
    right = (int64_t)x + width;
    bottom = (int64_t)y + height;
    if (right <= 0 || bottom <= 0 || left >= CRUMB_FRAMEBUFFER_WIDTH ||
        top >= CRUMB_FRAMEBUFFER_HEIGHT) {
        return;
    }
    if (left < 0) {
        left = 0;
    }
    if (top < 0) {
        top = 0;
    }
    if (right > CRUMB_FRAMEBUFFER_WIDTH) {
        right = CRUMB_FRAMEBUFFER_WIDTH;
    }
    if (bottom > CRUMB_FRAMEBUFFER_HEIGHT) {
        bottom = CRUMB_FRAMEBUFFER_HEIGHT;
    }

    clamped_red = clamp_color(red);
    clamped_green = clamp_color(green);
    clamped_blue = clamp_color(blue);
    for (draw_y = (int)top; draw_y < (int)bottom; ++draw_y) {
        for (draw_x = (int)left; draw_x < (int)right; ++draw_x) {
            write_pixel(draw_x, draw_y, clamped_red, clamped_green, clamped_blue);
        }
    }
}

const unsigned char *crumb_framebuffer_pixels(void) { return pixels; }
