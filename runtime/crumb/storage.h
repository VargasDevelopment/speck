#ifndef CRUMB_STORAGE_H
#define CRUMB_STORAGE_H

#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define CRUMB_STORAGE_SLOT_COUNT 16

/*
 * Selects the game's private storage namespace. All length bytes of identity
 * are hashed rather than used as a path, including embedded NULs. An empty
 * identity is valid. Calling this again switches namespaces; callers must
 * not race initialization with loads or saves.
 */
void crumb_storage_init(const char *identity, uint64_t length);

/*
 * Slots outside [0, CRUMB_STORAGE_SLOT_COUNT) fail without doing I/O. Saves
 * atomically replace one slot, so concurrent writers use last-rename-wins
 * semantics and never overwrite other slots.
 */
bool crumb_save_i32(int32_t slot, int32_t value);
int32_t crumb_load_i32(int32_t slot, int32_t fallback);

#ifdef __cplusplus
}
#endif

#endif
