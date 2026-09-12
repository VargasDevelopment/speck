#include "crumb.h"
#include "audio_internal.h"

#include <math.h>
#include <stdatomic.h>
#include <string.h>

_Static_assert(ATOMIC_INT_LOCK_FREE == 2, "audio needs lock-free integer atomics");

enum { COMMAND_SLOTS = CRUMB_AUDIO_COMMANDS + 1 };

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

static struct effect commands[COMMAND_SLOTS];
static atomic_uint read_index;
static atomic_uint write_index;
static atomic_int enabled;
/* Only the callback owns voices and the noise seed. */
static struct voice voices[CRUMB_AUDIO_VOICES];
static uint32_t noise_seed;

void crumb_audio_enable(int value) {
    atomic_store_explicit(&enabled, value, memory_order_release);
}

void crumb_audio_reset(void) {
    crumb_audio_enable(0);
    atomic_store_explicit(&read_index, 0, memory_order_relaxed);
    atomic_store_explicit(&write_index, 0, memory_order_relaxed);
    memset(voices, 0, sizeof voices);
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
    for (size_t sample = 0; sample < count; ++sample) {
        float mixed = 0.0f;
        for (unsigned int i = 0; i < CRUMB_AUDIO_VOICES; ++i) {
            if (voices[i].position < voices[i].effect.samples) {
                mixed += next_sample(&voices[i]);
            }
        }
        samples[sample] = (int16_t)(fmaxf(-1.0f, fminf(mixed, 1.0f)) * 32767.0f);
    }
}
