#include "crumb.h"
#include "crumb_internal.h"

#include <string.h>

static unsigned char key_down_state[CRUMB_KEY_COUNT];
static unsigned char key_pressed_state[CRUMB_KEY_COUNT];
static unsigned char key_released_state[CRUMB_KEY_COUNT];
static int quit_requested = 0;

static int valid_key(int key) { return key >= 0 && key < CRUMB_KEY_COUNT; }

void crumb_input_reset(void) {
    memset(key_down_state, 0, sizeof(key_down_state));
    memset(key_pressed_state, 0, sizeof(key_pressed_state));
    memset(key_released_state, 0, sizeof(key_released_state));
    quit_requested = 0;
}

void crumb_input_begin_frame(void) {
    memset(key_pressed_state, 0, sizeof(key_pressed_state));
    memset(key_released_state, 0, sizeof(key_released_state));
}

void crumb_input_set_key(int key, int is_down) {
    if (!valid_key(key)) {
        return;
    }
    if (is_down && !key_down_state[key]) {
        key_down_state[key] = 1;
        key_pressed_state[key] = 1;
    } else if (!is_down && key_down_state[key]) {
        key_down_state[key] = 0;
        key_released_state[key] = 1;
    }
}

void crumb_input_release_all(void) {
    int key;

    for (key = 0; key < CRUMB_KEY_COUNT; ++key) {
        crumb_input_set_key(key, 0);
    }
}

bool crumb_key_down(int key) { return valid_key(key) && key_down_state[key] != 0; }

bool crumb_key_pressed(int key) { return valid_key(key) && key_pressed_state[key] != 0; }

bool crumb_key_released(int key) { return valid_key(key) && key_released_state[key] != 0; }

void crumb_request_quit(void) { quit_requested = 1; }

int crumb_quit_requested(void) { return quit_requested; }
