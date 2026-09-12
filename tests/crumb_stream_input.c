#include "../runtime/crumb/present_stream.c"

#include <assert.h>
#include <stdio.h>

static void poll_record(FILE *records, int sender) {
    unsigned char bytes[CRUMB_INPUT_MESSAGE_BYTES];
    assert(fread(bytes, sizeof(bytes), 1, records) == 1);
    assert(write(sender, bytes, sizeof(bytes)) == sizeof(bytes));
    assert(crumb_present_poll() == CRUMB_PRESENT_CONTINUE);
}

int main(int argc, char **argv) {
    int sockets[2];
    FILE *records;
    assert(argc == 2);
    records = fopen(argv[1], "rb");
    assert(records != NULL);
    assert(socketpair(AF_UNIX, SOCK_STREAM, 0, sockets) == 0);
    stream_socket = sockets[1];
    crumb_input_reset();

    // Every Rust browser mapping must reach its exact C input slot. Exercise
    // transitions and repeat suppression, then simultaneous held-key release.
    for (int key = 0; key < CRUMB_KEY_COUNT; ++key) {
        poll_record(records, sockets[0]);
        assert(crumb_key_down(key));
        assert(crumb_key_pressed(key));
        crumb_input_begin_frame();
        poll_record(records, sockets[0]);
        assert(crumb_key_down(key));
        assert(!crumb_key_pressed(key));
        poll_record(records, sockets[0]);
        assert(!crumb_key_down(key));
        assert(crumb_key_released(key));
        crumb_input_begin_frame();
    }
    for (int key = 0; key < CRUMB_KEY_COUNT; ++key) {
        poll_record(records, sockets[0]);
        assert(crumb_key_down(key));
    }
    crumb_input_begin_frame();
    poll_record(records, sockets[0]);
    for (int key = 0; key < CRUMB_KEY_COUNT; ++key) {
        assert(!crumb_key_down(key));
        assert(crumb_key_released(key));
    }
    crumb_input_begin_frame();
    poll_record(records, sockets[0]);
    for (int key = 0; key < CRUMB_KEY_COUNT; ++key) {
        assert(!crumb_key_down(key));
        assert(!crumb_key_pressed(key));
        assert(!crumb_key_released(key));
    }
    assert(fgetc(records) == EOF);
    fclose(records);
    close(sockets[0]);
    crumb_present_shutdown();
    return 0;
}
