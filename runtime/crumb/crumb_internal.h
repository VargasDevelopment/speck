#ifndef CRUMB_INTERNAL_H
#define CRUMB_INTERNAL_H

enum crumb_present_result {
    CRUMB_PRESENT_ERROR = -1,
    CRUMB_PRESENT_CONTINUE = 0,
    CRUMB_PRESENT_STOP = 1
};

enum crumb_key {
    CRUMB_KEY_W = 0,
    CRUMB_KEY_A = 1,
    CRUMB_KEY_S = 2,
    CRUMB_KEY_D = 3,
    CRUMB_KEY_UP = 4,
    CRUMB_KEY_DOWN = 5,
    CRUMB_KEY_LEFT = 6,
    CRUMB_KEY_RIGHT = 7,
    CRUMB_KEY_SPACE = 8,
    CRUMB_KEY_ENTER = 9,
    CRUMB_KEY_ESCAPE = 10,
    CRUMB_KEY_COUNT = 11
};

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
