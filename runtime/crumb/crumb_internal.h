#ifndef CRUMB_INTERNAL_H
#define CRUMB_INTERNAL_H

enum crumb_present_result {
    CRUMB_PRESENT_ERROR = -1,
    CRUMB_PRESENT_CONTINUE = 0,
    CRUMB_PRESENT_STOP = 1
};

enum crumb_key {
#define SPECK_KEY(suffix, variant, id, browser, macos, modifier) CRUMB_KEY_##suffix = id,
#include "keys.def"
#undef SPECK_KEY
    CRUMB_KEY_COUNT
};

void crumb_audio_init(void);
void crumb_audio_shutdown(void);

int crumb_present_init(void);
int crumb_present_poll(void);
int crumb_present(void);
void crumb_present_shutdown(void);

void crumb_input_reset(void);
void crumb_input_begin_frame(void);
void crumb_input_set_key(int key, int is_down);
void crumb_input_release_all(void);
int crumb_quit_requested(void);

int crumb_platform_init(void);
int crumb_platform_should_stop(void);
void crumb_platform_shutdown(void);

#endif
