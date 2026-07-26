#ifndef CRUMB_H
#define CRUMB_H

#ifdef __cplusplus
extern "C" {
#endif

#define CRUMB_ABI_VERSION 0
#define CRUMB_FRAMEBUFFER_WIDTH 320
#define CRUMB_FRAMEBUFFER_HEIGHT 180
#define CRUMB_FRAMEBUFFER_CHANNELS 3
#define CRUMB_FRAMEBUFFER_STRIDE (CRUMB_FRAMEBUFFER_WIDTH * CRUMB_FRAMEBUFFER_CHANNELS)
#define CRUMB_FRAMEBUFFER_BYTES (CRUMB_FRAMEBUFFER_STRIDE * CRUMB_FRAMEBUFFER_HEIGHT)

/* Speck entry points supplied by the generated game object. */
void spk_start(void);
void spk_update(float dt);
void spk_draw(void);

/* CRuMB v0 platform and debug API. */
int crumb_init(void);
float crumb_frame_delta(void);
void crumb_print_i32(int value);
void crumb_debug_frame(int frame, float value);
void crumb_clear_rgb(int red, int green, int blue);
void crumb_fill_rect(int x, int y, int width, int height, int red, int green, int blue);
const unsigned char *crumb_framebuffer_pixels(void);
void crumb_shutdown(void);
int crumb_run(void);

#ifdef __cplusplus
}
#endif

#endif
