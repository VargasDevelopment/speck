#include "storage.h"

#include <errno.h>
#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int failure(const char *message) {
    fprintf(stderr, "storage test failed: %s\n", message);
    return 1;
}

static int parse_i32(const char *text, int32_t *value) {
    char *end = NULL;
    intmax_t parsed;

    errno = 0;
    parsed = strtoimax(text, &end, 10);
    if (errno != 0 || end == text || *end != '\0' ||
        parsed < INT32_MIN || parsed > INT32_MAX) {
        return 0;
    }
    *value = (int32_t)parsed;
    return 1;
}

int main(int argc, char **argv) {
    int32_t slot;
    int32_t value;
    int32_t fallback;

    if (argc < 3) {
        return failure("expected a command and identity");
    }
    crumb_storage_init(argv[2], (uint64_t)strlen(argv[2]));

    if (strcmp(argv[1], "write-fixture") == 0) {
        if (crumb_save_i32(-1, 1) || crumb_save_i32(CRUMB_STORAGE_SLOT_COUNT, 1)) {
            return failure("an out-of-range save succeeded");
        }
        if (crumb_load_i32(-1, 71) != 71 ||
            crumb_load_i32(CRUMB_STORAGE_SLOT_COUNT, 72) != 72) {
            return failure("an out-of-range load ignored its fallback");
        }
        if (!crumb_save_i32(0, INT32_MIN) ||
            !crumb_save_i32(1, INT32_MAX) ||
            !crumb_save_i32(2, -123456789) ||
            !crumb_save_i32(3, 0)) {
            return failure("could not write the fixture");
        }
        return 0;
    }
    if (strcmp(argv[1], "read-fixture") == 0) {
        if (crumb_load_i32(0, 1) != INT32_MIN ||
            crumb_load_i32(1, 1) != INT32_MAX ||
            crumb_load_i32(2, 1) != -123456789 ||
            crumb_load_i32(3, 1) != 0) {
            return failure("fixture values did not survive restart");
        }
        return 0;
    }
    if (strcmp(argv[1], "put") == 0) {
        if (argc != 5 || !parse_i32(argv[3], &slot) || !parse_i32(argv[4], &value)) {
            return failure("invalid put arguments");
        }
        return crumb_save_i32(slot, value) ? 0 : failure("save failed");
    }
    if (strcmp(argv[1], "get") == 0) {
        int32_t expected;
        if (argc != 6 || !parse_i32(argv[3], &slot) ||
            !parse_i32(argv[4], &expected) || !parse_i32(argv[5], &fallback)) {
            return failure("invalid get arguments");
        }
        return crumb_load_i32(slot, fallback) == expected
                   ? 0
                   : failure("load returned an unexpected value");
    }
    if (strcmp(argv[1], "expect-failure") == 0) {
        if (crumb_save_i32(0, 99) || crumb_load_i32(0, 88) != 88) {
            return failure("unavailable storage did not fail gracefully");
        }
        return 0;
    }
    return failure("unknown command");
}
