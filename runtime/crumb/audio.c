#include "crumb.h"
#include "audio_internal.h"

#include <math.h>
#include <stdatomic.h>
#include <string.h>

_Static_assert(ATOMIC_INT_LOCK_FREE == 2, "audio needs lock-free integer atomics");

enum { COMMAND_SLOTS = CRUMB_AUDIO_COMMANDS + 1 };
enum { SOUND_COMMAND_SLOTS = 17 };

enum sound_command_kind {
    SOUND_PLAY,
    SOUND_SEEK
};

struct effect {
    float frequency;
    float volume;
    unsigned int samples;
};

struct voice {
    struct effect effect;
    unsigned int position;
    float phase;
    uint32_t random;
};

struct sound_command {
    enum sound_command_kind kind;
    const int16_t *samples;
    unsigned int sample_count;
    unsigned int position;
    unsigned int epoch;
    float volume;
};

struct backing_track {
    const int16_t *samples;
    unsigned int sample_count;
    unsigned int position;
    unsigned int epoch;
    float volume;
};

extern int spk_sound_lookup(int handle, const int16_t **samples, unsigned int *sample_count);
#ifdef CRUMB_AUDIO_TEST_HOOKS
extern void crumb_audio_test_before_sound_play_publish(void);
#endif

static struct effect commands[COMMAND_SLOTS];
static atomic_uint read_index;
static atomic_uint write_index;
static atomic_int enabled;
/* Only the callback owns voices and the noise seed. */
static struct voice voices[CRUMB_AUDIO_VOICES];
static uint32_t noise_seed;
static struct sound_command sound_commands[SOUND_COMMAND_SLOTS];
static atomic_uint sound_read_index;
static atomic_uint sound_write_index;
static atomic_uint sound_epoch;
static atomic_uint sound_position;
static atomic_int sound_paused;
static atomic_int sound_stopped;
/* Only the callback owns the backing track. */
static struct backing_track backing_track;

void crumb_audio_enable(int value) {
    atomic_store_explicit(&enabled, value, memory_order_release);
}

void crumb_audio_reset(void) {
    crumb_audio_enable(0);
    atomic_store_explicit(&read_index, 0, memory_order_relaxed);
    atomic_store_explicit(&write_index, 0, memory_order_relaxed);
    atomic_store_explicit(&sound_read_index, 0, memory_order_relaxed);
    atomic_store_explicit(&sound_write_index, 0, memory_order_relaxed);
    atomic_store_explicit(&sound_epoch, 0, memory_order_relaxed);
    atomic_store_explicit(&sound_position, 0, memory_order_relaxed);
    atomic_store_explicit(&sound_paused, 0, memory_order_relaxed);
    atomic_store_explicit(&sound_stopped, 1, memory_order_relaxed);
    memset(voices, 0, sizeof voices);
    memset(&backing_track, 0, sizeof backing_track);
    noise_seed = UINT32_C(0x12345678);
}

static void submit(float frequency, float seconds, float volume) {
    unsigned int write;
    unsigned int next;
    struct effect effect;

    if (!atomic_load_explicit(&enabled, memory_order_acquire) ||
        !isfinite(seconds) || !isfinite(volume) || seconds <= 0.0f || volume <= 0.0f) {
        return;
    }
    effect.frequency = frequency;
    effect.volume = fminf(volume, 1.0f);
    effect.samples = (unsigned int)(fminf(seconds, 2.0f) * CRUMB_AUDIO_SAMPLE_RATE);
    if (effect.samples < 2) {
        return;
    }
    write = atomic_load_explicit(&write_index, memory_order_relaxed);
    next = (write + 1) % COMMAND_SLOTS;
    if (next == atomic_load_explicit(&read_index, memory_order_acquire)) {
        return;
    }
    commands[write] = effect;
    atomic_store_explicit(&write_index, next, memory_order_release);
}

void crumb_tone(float frequency, float seconds, float volume) {
    if (isfinite(frequency) && frequency >= 20.0f && frequency <= 20000.0f) {
        submit(frequency, seconds, volume);
    }
}

void crumb_noise(float seconds, float volume) {
    submit(0.0f, seconds, volume);
}

static int submit_sound(struct sound_command command) {
    const unsigned int write = atomic_load_explicit(&sound_write_index, memory_order_relaxed);
    const unsigned int next = (write + 1) % SOUND_COMMAND_SLOTS;

    if (!atomic_load_explicit(&enabled, memory_order_acquire) ||
        next == atomic_load_explicit(&sound_read_index, memory_order_acquire)) {
        return 0;
    }
    sound_commands[write] = command;
    if (command.kind == SOUND_PLAY) {
        /* A callback may consume the command only after its transport state is visible. */
        atomic_store_explicit(&sound_paused, 0, memory_order_release);
        atomic_store_explicit(&sound_stopped, 0, memory_order_release);
#ifdef CRUMB_AUDIO_TEST_HOOKS
        crumb_audio_test_before_sound_play_publish();
#endif
    }
    atomic_store_explicit(&sound_write_index, next, memory_order_release);
    return 1;
}

void crumb_sound_play(int handle, float volume) {
    struct sound_command command;

    if (!isfinite(volume) || volume <= 0.0f ||
        !spk_sound_lookup(handle, &command.samples, &command.sample_count) ||
        command.samples == NULL || command.sample_count == 0) {
        return;
    }
    command.kind = SOUND_PLAY;
    command.position = 0;
    command.epoch = atomic_load_explicit(&sound_epoch, memory_order_acquire);
    command.volume = fminf(volume, 1.0f);
    (void)submit_sound(command);
}

void crumb_sound_pause(void) {
    atomic_store_explicit(&sound_paused, 1, memory_order_release);
}

void crumb_sound_resume(void) {
    atomic_store_explicit(&sound_paused, 0, memory_order_release);
}

void crumb_sound_stop(void) {
    atomic_fetch_add_explicit(&sound_epoch, 1, memory_order_acq_rel);
    atomic_store_explicit(&sound_stopped, 1, memory_order_release);
    atomic_store_explicit(&sound_position, 0, memory_order_release);
}

float crumb_sound_position(void) {
    if (!atomic_load_explicit(&enabled, memory_order_acquire) ||
        atomic_load_explicit(&sound_stopped, memory_order_acquire)) {
        return 0.0f;
    }
    return (float)atomic_load_explicit(&sound_position, memory_order_acquire) /
           (float)CRUMB_AUDIO_SAMPLE_RATE;
}

void crumb_sound_seek(float seconds) {
    struct sound_command command;
    float bounded;

    if (!isfinite(seconds)) {
        return;
    }
    bounded = fmaxf(0.0f, fminf(seconds, 180.0f));
    command.kind = SOUND_SEEK;
    command.samples = NULL;
    command.sample_count = 0;
    command.position = (unsigned int)(bounded * CRUMB_AUDIO_SAMPLE_RATE);
    command.epoch = atomic_load_explicit(&sound_epoch, memory_order_acquire);
    command.volume = 0.0f;
    (void)submit_sound(command);
}

static void receive_sound_commands(void) {
    unsigned int read = atomic_load_explicit(&sound_read_index, memory_order_relaxed);
    const unsigned int end = atomic_load_explicit(&sound_write_index, memory_order_acquire);
    const unsigned int epoch = atomic_load_explicit(&sound_epoch, memory_order_acquire);

    while (read != end) {
        const struct sound_command command = sound_commands[read];
        if (command.epoch == epoch) {
            if (command.kind == SOUND_PLAY) {
                backing_track.samples = command.samples;
                backing_track.sample_count = command.sample_count;
                backing_track.position = 0;
                backing_track.epoch = command.epoch;
                backing_track.volume = command.volume;
            } else if (backing_track.samples != NULL) {
                backing_track.position = command.position < backing_track.sample_count
                                             ? command.position
                                             : backing_track.sample_count;
            }
        }
        read = (read + 1) % SOUND_COMMAND_SLOTS;
        atomic_store_explicit(&sound_read_index, read, memory_order_release);
    }
}

static void receive_commands(void) {
    unsigned int read = atomic_load_explicit(&read_index, memory_order_relaxed);
    /* Snapshot bounds callback work even if the producer continues submitting. */
    const unsigned int end = atomic_load_explicit(&write_index, memory_order_acquire);

    while (read != end) {
        for (unsigned int i = 0; i < CRUMB_AUDIO_VOICES; ++i) {
            if (voices[i].position >= voices[i].effect.samples) {
                voices[i].effect = commands[read];
                voices[i].position = 0;
                voices[i].phase = 0.0f;
                noise_seed = noise_seed * UINT32_C(1664525) + UINT32_C(1013904223);
                voices[i].random = noise_seed;
                break;
            }
        }
        /* Full voice capacity drops the new effect instead of cutting a sound. */
        read = (read + 1) % COMMAND_SLOTS;
        atomic_store_explicit(&read_index, read, memory_order_release);
    }
}

static float next_sample(struct voice *voice) {
    const float tau = 6.2831853071795864769f;
    const unsigned int length = voice->effect.samples;
    const unsigned int position = voice->position++;
    const unsigned int attack = length / 2 < 96 ? length / 2 : 96;
    const float onset = fminf((float)position / (float)attack, 1.0f);
    const float decay = (float)(length - 1 - position) / (float)(length - 1);
    float wave;

    if (voice->effect.frequency == 0.0f) {
        voice->random = voice->random * UINT32_C(1664525) + UINT32_C(1013904223);
        wave = (float)(voice->random >> 8) / 8388607.5f - 1.0f;
    } else {
        wave = sinf(voice->phase);
        voice->phase += tau * voice->effect.frequency / CRUMB_AUDIO_SAMPLE_RATE;
        if (voice->phase >= tau) {
            voice->phase -= tau;
        }
    }
    /* Leave room for overlapping effects, with a short onset and percussive decay. */
    return wave * voice->effect.volume * 0.25f * onset * decay;
}

void crumb_audio_render(int16_t *samples, size_t count) {
    if (!atomic_load_explicit(&enabled, memory_order_acquire)) {
        memset(samples, 0, count * sizeof *samples);
        return;
    }
    receive_commands();
    receive_sound_commands();
    if (atomic_load_explicit(&sound_stopped, memory_order_acquire) ||
        backing_track.epoch != atomic_load_explicit(&sound_epoch, memory_order_acquire)) {
        memset(&backing_track, 0, sizeof backing_track);
    }
    for (size_t sample = 0; sample < count; ++sample) {
        float mixed = 0.0f;
        if (!atomic_load_explicit(&sound_paused, memory_order_acquire) &&
            backing_track.position < backing_track.sample_count) {
            mixed += ((float)backing_track.samples[backing_track.position++] / 32768.0f) *
                     backing_track.volume;
        }
        for (unsigned int i = 0; i < CRUMB_AUDIO_VOICES; ++i) {
            if (voices[i].position < voices[i].effect.samples) {
                mixed += next_sample(&voices[i]);
            }
        }
        samples[sample] = (int16_t)(fmaxf(-1.0f, fminf(mixed, 1.0f)) * 32767.0f);
    }
    atomic_store_explicit(&sound_position, backing_track.position, memory_order_release);
}
