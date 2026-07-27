#include "crumb.h"
#include "crumb_internal.h"

#include <assert.h>

int main(void) {
    crumb_input_reset();
    assert(!crumb_key_down(CRUMB_KEY_A));
    assert(!crumb_key_pressed(CRUMB_KEY_A));
    assert(!crumb_key_released(CRUMB_KEY_A));

    crumb_input_begin_frame();
    crumb_input_set_key(CRUMB_KEY_A, 1);
    assert(crumb_key_down(CRUMB_KEY_A));
    assert(crumb_key_pressed(CRUMB_KEY_A));
    assert(!crumb_key_released(CRUMB_KEY_A));

    crumb_input_begin_frame();
    assert(crumb_key_down(CRUMB_KEY_A));
    assert(!crumb_key_pressed(CRUMB_KEY_A));
    crumb_input_set_key(CRUMB_KEY_A, 1);
    assert(!crumb_key_pressed(CRUMB_KEY_A));

    crumb_input_set_key(CRUMB_KEY_A, 0);
    assert(!crumb_key_down(CRUMB_KEY_A));
    assert(crumb_key_released(CRUMB_KEY_A));
    crumb_input_set_key(CRUMB_KEY_A, 0);
    assert(crumb_key_released(CRUMB_KEY_A));

    crumb_input_begin_frame();
    crumb_input_set_key(CRUMB_KEY_SPACE, 1);
    crumb_input_set_key(CRUMB_KEY_SPACE, 0);
    assert(!crumb_key_down(CRUMB_KEY_SPACE));
    assert(crumb_key_pressed(CRUMB_KEY_SPACE));
    assert(crumb_key_released(CRUMB_KEY_SPACE));

    crumb_input_begin_frame();
    crumb_input_set_key(CRUMB_KEY_W, 1);
    crumb_input_set_key(CRUMB_KEY_RIGHT, 1);
    assert(crumb_key_down(CRUMB_KEY_W));
    assert(crumb_key_down(CRUMB_KEY_RIGHT));
    assert(!crumb_key_down(CRUMB_KEY_S));
    crumb_input_release_all();
    assert(!crumb_key_down(CRUMB_KEY_W));
    assert(!crumb_key_down(CRUMB_KEY_RIGHT));
    assert(crumb_key_released(CRUMB_KEY_W));
    assert(crumb_key_released(CRUMB_KEY_RIGHT));

    crumb_input_begin_frame();
    crumb_input_set_key(-1, 1);
    crumb_input_set_key(CRUMB_KEY_COUNT, 1);
    crumb_input_set_key(999, 1);
    assert(!crumb_key_down(-1));
    assert(!crumb_key_pressed(CRUMB_KEY_COUNT));
    assert(!crumb_key_released(999));

    assert(!crumb_quit_requested());
    crumb_request_quit();
    assert(crumb_quit_requested());
    crumb_input_reset();
    assert(!crumb_quit_requested());

    return 0;
}
