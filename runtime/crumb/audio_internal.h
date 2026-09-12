#ifndef CRUMB_AUDIO_INTERNAL_H
#define CRUMB_AUDIO_INTERNAL_H

#include <stddef.h>
#include <stdint.h>

enum {
    CRUMB_AUDIO_SAMPLE_RATE = 48000,
    CRUMB_AUDIO_VOICES = 8,
    CRUMB_AUDIO_COMMANDS = 32
};

/* The game thread submits effects; one audio callback thread renders them.
 * Reset is only legal before starting callbacks or after synchronous disposal. */
void crumb_audio_reset(void);
void crumb_audio_enable(int enabled);
void crumb_audio_render(int16_t *samples, size_t count);

#endif
