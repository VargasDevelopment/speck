#if defined(__APPLE__)
#define _DARWIN_C_SOURCE
#endif
#define _POSIX_C_SOURCE 200809L

#include "storage.h"

#include <errno.h>
#include <fcntl.h>
#include <inttypes.h>
#include <limits.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>

static const char CRUMB_STORAGE_HEADER[] = "SPECK-I32-V1\n";
static char *crumb_storage_directory;

static int crumb_storage_valid_slot(int32_t slot) {
    return slot >= 0 && slot < CRUMB_STORAGE_SLOT_COUNT;
}

static uint64_t crumb_storage_identity_hash(const char *identity, uint64_t length) {
    const unsigned char *cursor = (const unsigned char *)identity;
    uint64_t hash = UINT64_C(14695981039346656037);

    for (uint64_t index = 0; index < length; ++index) {
        hash ^= (uint64_t)*cursor;
        hash *= UINT64_C(1099511628211);
        ++cursor;
    }
    return hash;
}

static char *crumb_storage_build_directory(const char *base, const char *suffix,
                                           uint64_t identity_hash) {
    static const size_t game_component_length = sizeof("game-0123456789abcdef") - 1;
    const size_t base_length = strlen(base);
    const size_t suffix_length = strlen(suffix);
    const int needs_base_separator = base_length > 0 && base[base_length - 1] != '/';
    size_t length;
    char *path;
    int result;

    if (base_length > SIZE_MAX - suffix_length - game_component_length - 3) {
        return NULL;
    }
    length = base_length + (size_t)needs_base_separator + suffix_length +
             (suffix_length > 0 ? 1 : 0) + game_component_length + 1;
    path = (char *)malloc(length);
    if (path == NULL) {
        return NULL;
    }
    if (suffix_length > 0) {
        result = snprintf(path, length, "%s%s%s/game-%016" PRIx64, base,
                          needs_base_separator ? "/" : "", suffix, identity_hash);
    } else {
        result = snprintf(path, length, "%s%sgame-%016" PRIx64, base,
                          needs_base_separator ? "/" : "", identity_hash);
    }
    if (result < 0 || (size_t)result >= length) {
        free(path);
        return NULL;
    }
    return path;
}

void crumb_storage_init(const char *identity, uint64_t length) {
    const char *base;
    const char *override;
    const char *suffix;

    free(crumb_storage_directory);
    crumb_storage_directory = NULL;
    if (identity == NULL) {
        return;
    }

    override = getenv("SPECK_SAVE_DIR");
    if (override != NULL && override[0] != '\0') {
        base = override;
        suffix = "";
    } else {
#if defined(__APPLE__)
        base = getenv("HOME");
        suffix = "Library/Application Support/Speck";
#else
        const char *xdg_data_home = getenv("XDG_DATA_HOME");

        if (xdg_data_home != NULL && xdg_data_home[0] == '/') {
            base = xdg_data_home;
            suffix = "speck";
        } else {
            base = getenv("HOME");
            suffix = ".local/share/speck";
        }
#endif
    }

    /* Absolute roots keep storage independent from the process CWD. */
    if (base == NULL || base[0] != '/') {
        return;
    }
    crumb_storage_directory = crumb_storage_build_directory(
        base, suffix, crumb_storage_identity_hash(identity, length));
}

static int crumb_storage_ensure_directory(const char *path) {
    char *copy;
    char *cursor;
    struct stat metadata;
    size_t length;

    length = strlen(path);
    copy = (char *)malloc(length + 1);
    if (copy == NULL) {
        return 0;
    }
    memcpy(copy, path, length + 1);
    for (cursor = copy + 1; ; ++cursor) {
        if (*cursor != '/' && *cursor != '\0') {
            continue;
        }
        if (cursor != copy + 1) {
            char saved = *cursor;
            *cursor = '\0';
            if (mkdir(copy, S_IRWXU) != 0) {
                if (errno != EEXIST || stat(copy, &metadata) != 0 ||
                    !S_ISDIR(metadata.st_mode)) {
                    free(copy);
                    return 0;
                }
            }
            *cursor = saved;
        }
        if (*cursor == '\0') {
            break;
        }
    }
    free(copy);
    return 1;
}

static int crumb_storage_open_directory(int create) {
    int directory;
    struct stat metadata;

    if (crumb_storage_directory == NULL) {
        return -1;
    }
    if (create && !crumb_storage_ensure_directory(crumb_storage_directory)) {
        return -1;
    }
    directory = open(crumb_storage_directory,
                     O_RDONLY | O_DIRECTORY | O_CLOEXEC | O_NOFOLLOW);
    if (directory < 0) {
        return -1;
    }
    if (fstat(directory, &metadata) != 0 || !S_ISDIR(metadata.st_mode)) {
        (void)close(directory);
        return -1;
    }
    return directory;
}

static void crumb_storage_slot_name(char name[static 12], int32_t slot) {
    (void)snprintf(name, 12, "slot-%02" PRId32 ".i32", slot);
}

static int crumb_storage_write_all(int descriptor, const char *data, size_t length) {
    size_t written = 0;

    while (written < length) {
        ssize_t result = write(descriptor, data + written, length - written);
        if (result > 0) {
            written += (size_t)result;
        } else if (result < 0 && errno == EINTR) {
            continue;
        } else {
            return 0;
        }
    }
    return 1;
}

bool crumb_save_i32(int32_t slot, int32_t value) {
    char slot_name[12];
    char temporary_name[80];
    char contents[40];
    int content_length;
    int directory;
    int temporary = -1;
    int write_succeeded;
    unsigned int attempt;
    int saved_errno;

    if (!crumb_storage_valid_slot(slot)) {
        return false;
    }
    directory = crumb_storage_open_directory(1);
    if (directory < 0) {
        return false;
    }
    crumb_storage_slot_name(slot_name, slot);
    content_length = snprintf(contents, sizeof(contents), "%s%" PRId32 "\n",
                              CRUMB_STORAGE_HEADER, value);
    if (content_length < 0 || (size_t)content_length >= sizeof(contents)) {
        (void)close(directory);
        return false;
    }

    for (attempt = 0; attempt < 128; ++attempt) {
        int name_length = snprintf(temporary_name, sizeof(temporary_name),
                                   ".slot-%02" PRId32 ".tmp-%" PRIdMAX "-%u",
                                   slot, (intmax_t)getpid(), attempt);
        if (name_length < 0 || (size_t)name_length >= sizeof(temporary_name)) {
            (void)close(directory);
            return false;
        }
        temporary = openat(directory, temporary_name,
                           O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC | O_NOFOLLOW,
                           S_IRUSR | S_IWUSR);
        if (temporary >= 0) {
            break;
        }
        if (errno != EEXIST) {
            (void)close(directory);
            return false;
        }
    }
    if (temporary < 0) {
        (void)close(directory);
        return false;
    }

    write_succeeded =
        crumb_storage_write_all(temporary, contents, (size_t)content_length) &&
        fsync(temporary) == 0;
    if (close(temporary) != 0) {
        write_succeeded = 0;
    }
    if (!write_succeeded) {
        saved_errno = errno;
        (void)unlinkat(directory, temporary_name, 0);
        (void)close(directory);
        errno = saved_errno;
        return false;
    }
    if (renameat(directory, temporary_name, directory, slot_name) != 0) {
        saved_errno = errno;
        (void)unlinkat(directory, temporary_name, 0);
        (void)close(directory);
        errno = saved_errno;
        return false;
    }
    /* The value is already atomically visible; sync the directory when supported. */
    (void)fsync(directory);
    (void)close(directory);
    return true;
}

static int crumb_storage_read_all(int descriptor, char *data, size_t capacity,
                                  size_t *length) {
    size_t used = 0;

    while (used < capacity) {
        ssize_t result = read(descriptor, data + used, capacity - used);
        if (result > 0) {
            used += (size_t)result;
        } else if (result == 0) {
            *length = used;
            return 1;
        } else if (errno != EINTR) {
            return 0;
        }
    }
    return 0;
}

static int crumb_storage_parse_i32(const char *data, size_t length, int32_t *value) {
    const size_t header_length = sizeof(CRUMB_STORAGE_HEADER) - 1;
    size_t cursor;
    size_t digit_start;
    uint32_t magnitude = 0;
    uint32_t limit;
    int negative = 0;

    if (length <= header_length + 1 ||
        memcmp(data, CRUMB_STORAGE_HEADER, header_length) != 0 ||
        data[length - 1] != '\n') {
        return 0;
    }
    cursor = header_length;
    if (data[cursor] == '-') {
        negative = 1;
        ++cursor;
    }
    digit_start = cursor;
    if (cursor >= length - 1 ||
        (length - 1 - cursor > 1 && data[cursor] == '0')) {
        return 0;
    }
    limit = negative ? UINT32_C(2147483648) : UINT32_C(2147483647);
    while (cursor < length - 1) {
        uint32_t digit;
        if (data[cursor] < '0' || data[cursor] > '9') {
            return 0;
        }
        digit = (uint32_t)(data[cursor] - '0');
        if (magnitude > (limit - digit) / UINT32_C(10)) {
            return 0;
        }
        magnitude = magnitude * UINT32_C(10) + digit;
        ++cursor;
    }
    if (cursor == digit_start || (negative && magnitude == 0)) {
        return 0;
    }
    if (negative) {
        *value = (int32_t)(-(int64_t)magnitude);
    } else {
        *value = (int32_t)magnitude;
    }
    return 1;
}

int32_t crumb_load_i32(int32_t slot, int32_t fallback) {
    char slot_name[12];
    char contents[64];
    size_t content_length;
    int directory;
    int descriptor;
    int read_succeeded;
    int32_t value;
    struct stat metadata;

    if (!crumb_storage_valid_slot(slot)) {
        return fallback;
    }
    directory = crumb_storage_open_directory(0);
    if (directory < 0) {
        return fallback;
    }
    crumb_storage_slot_name(slot_name, slot);
    descriptor = openat(directory, slot_name,
                        O_RDONLY | O_CLOEXEC | O_NOFOLLOW | O_NONBLOCK);
    if (descriptor < 0) {
        (void)close(directory);
        return fallback;
    }
    read_succeeded =
        fstat(descriptor, &metadata) == 0 && S_ISREG(metadata.st_mode) &&
        metadata.st_size >= 0 && (uintmax_t)metadata.st_size < sizeof(contents) &&
        crumb_storage_read_all(descriptor, contents, sizeof(contents), &content_length);
    if (close(descriptor) != 0) {
        read_succeeded = 0;
    }
    if (!read_succeeded ||
        !crumb_storage_parse_i32(contents, content_length, &value)) {
        (void)close(directory);
        return fallback;
    }
    (void)close(directory);
    return value;
}
