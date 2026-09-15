#include "crumb.h"
#include "audio_internal.h"

#include <assert.h>
#include <float.h>
#include <limits.h>
#include <math.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdlib.h>
#include <string.h>

static int16_t samples[CRUMB_AUDIO_SAMPLE_RATE * 2 + 1];
static int16_t reference[CRUMB_AUDIO_SAMPLE_RATE * 2 + 1];
static const int16_t track[] = {32767, -32768, 16384, -16384, 8192, -8192, 1, -1};
static atomic_int hold_sound_publish;
static atomic_int sound_publish_waiting;

void crumb_audio_test_before_sound_play_publish(void) {
    if (atomic_load_explicit(&hold_sound_publish, memory_order_acquire)) {
        atomic_store_explicit(&sound_publish_waiting, 1, memory_order_release);
        while (atomic_load_explicit(&hold_sound_publish, memory_order_acquire)) {}
    }
}

int spk_sound_lookup(int handle, const int16_t **data, unsigned int *sample_count) {
    if (handle != 1) return 0;
    *data = track;
    *sample_count = sizeof track / sizeof *track;
    return 1;
}

static void reset(void) {
    crumb_audio_reset();
    crumb_audio_enable(1);
}

static void assert_silent(size_t count) {
    memset(samples, 0x7f, count * sizeof *samples);
    crumb_audio_render(samples, count);
    for (size_t i = 0; i < count; ++i) {
        assert(samples[i] == 0);
    }
}

static void tone_duration_frequency_and_envelope(void) {
    int crossings = 0;
    int early_peak = 0;
    int late_peak = 0;
    reset();
    crumb_tone(1000.0f, 0.1f, 1.0f);
    crumb_audio_render(samples, 4801);
    assert(samples[0] == 0 && samples[4799] == 0 && samples[4800] == 0);
    for (int i = 1; i < 4800; ++i) {
        crossings += samples[i - 1] <= 0 && samples[i] > 0;
        if (i < 2400 && abs(samples[i]) > early_peak) early_peak = abs(samples[i]);
        if (i >= 2400 && abs(samples[i]) > late_peak) late_peak = abs(samples[i]);
    }
    assert(crossings >= 99 && crossings <= 101);
    assert(early_peak > 7000 && early_peak <= 8192);
    assert(late_peak < early_peak / 2 + 100);
    assert_silent(256);
}

static void validation_and_clamps(void) {
    const float invalid[] = {NAN, INFINITY, -INFINITY, -1.0f, 0.0f};
    for (size_t i = 0; i < sizeof invalid / sizeof *invalid; ++i) {
        reset();
        crumb_tone(invalid[i], 0.1f, 1.0f);
        crumb_tone(1000.0f, invalid[i], 1.0f);
        crumb_tone(1000.0f, 0.1f, invalid[i]);
        crumb_noise(invalid[i], 1.0f);
        crumb_noise(0.1f, invalid[i]);
        assert_silent(256);
    }
    reset();
    crumb_tone(19.0f, 0.1f, 1.0f);
    crumb_tone(20001.0f, 0.1f, 1.0f);
    crumb_noise(FLT_MIN, 1.0f);
    assert_silent(256);
    reset();
    crumb_tone(1000.0f, FLT_MAX, FLT_MAX);
    crumb_audio_render(reference, CRUMB_AUDIO_SAMPLE_RATE * 2 + 1);
    reset();
    crumb_tone(1000.0f, 2.0f, 1.0f);
    crumb_audio_render(samples, CRUMB_AUDIO_SAMPLE_RATE * 2 + 1);
    assert(memcmp(samples, reference, sizeof samples) == 0);
    assert(samples[CRUMB_AUDIO_SAMPLE_RATE * 2 - 1] == 0);
    assert(samples[CRUMB_AUDIO_SAMPLE_RATE * 2] == 0);
}

static void deterministic_noise_and_chunk_boundaries(void) {
    int nonzero = 0;
    reset();
    crumb_noise(0.1f, 1.0f);
    crumb_audio_render(reference, 4801);
    reset();
    crumb_noise(0.1f, 1.0f);
    crumb_audio_render(samples, 173);
    crumb_audio_render(samples + 173, 4628);
    assert(memcmp(samples, reference, 4801 * sizeof *samples) == 0);
    for (int i = 0; i < 4800; ++i) nonzero += samples[i] != 0;
    assert(nonzero > 4700);
    assert(samples[0] == 0 && samples[4799] == 0 && samples[4800] == 0);
}

static void bounded_overload_and_reset(void) {
    reset();
    for (int i = 0; i < CRUMB_AUDIO_VOICES; ++i) crumb_tone(1000.0f, 0.01f, 1.0f);
    crumb_audio_render(reference, 512);
    reset();
    for (int i = 0; i < 10000; ++i) crumb_tone(1000.0f, 0.01f, 1.0f);
    crumb_audio_render(samples, 512);
    assert(memcmp(samples, reference, 512 * sizeof *samples) == 0);
    /* Overflow was dropped, not deferred until voices finish. */
    assert_silent(512);
    /* Repeated batches cross the ring's physical end and reuse expired voices. */
    for (int batch = 0; batch < 100; ++batch) {
        for (int i = 0; i < CRUMB_AUDIO_VOICES; ++i) crumb_tone(1000.0f, 0.01f, 1.0f);
        crumb_audio_render(samples, 512);
        assert(memcmp(samples, reference, 512 * sizeof *samples) == 0);
    }
    crumb_noise(2.0f, 1.0f);
    crumb_audio_render(samples, 256);
    crumb_noise(2.0f, 1.0f);
    crumb_audio_reset();
    assert_silent(256);
    crumb_audio_enable(1);
    assert_silent(256);
}

static atomic_int producer_done;

static void *produce(void *unused) {
    (void)unused;
    for (int i = 0; i < 100000; ++i) crumb_tone(20.0f + (float)(i % 19981), 0.003f, 1.0f);
    atomic_store(&producer_done, 1);
    return NULL;
}

static void concurrent_submission(void) {
    pthread_t producer;
    reset();
    assert(pthread_create(&producer, NULL, produce, NULL) == 0);
    do {
        crumb_audio_render(samples, 256);
    } while (!atomic_load(&producer_done));
    assert(pthread_join(producer, NULL) == 0);
    crumb_audio_render(samples, 256);
    assert_silent(256);
}

static int16_t track_sample(unsigned int index, float volume) {
    return (int16_t)(((float)track[index] / 32768.0f) * volume * 32767.0f);
}

static void pcm_track_transport_and_saturation(void) {
    reset();
    crumb_sound_play(1, 1.0f);
    crumb_audio_render(samples, 2);
    assert(samples[0] == track_sample(0, 1.0f));
    assert(samples[1] == track_sample(1, 1.0f));
    assert(fabsf(crumb_sound_position() - 2.0f / CRUMB_AUDIO_SAMPLE_RATE) < 0.000001f);

    crumb_sound_pause();
    assert_silent(3);
    assert(fabsf(crumb_sound_position() - 2.0f / CRUMB_AUDIO_SAMPLE_RATE) < 0.000001f);
    crumb_sound_resume();
    crumb_audio_render(samples, 2);
    assert(samples[0] == track_sample(2, 1.0f));
    assert(samples[1] == track_sample(3, 1.0f));

    crumb_sound_play(1, 0.5f);
    crumb_audio_render(samples, 1);
    assert(samples[0] == track_sample(0, 0.5f));
    crumb_sound_seek(5.0f / CRUMB_AUDIO_SAMPLE_RATE);
    crumb_audio_render(samples, 1);
    assert(samples[0] == track_sample(5, 0.5f));
    crumb_sound_seek(-INFINITY);
    crumb_audio_render(samples, 1);
    assert(samples[0] == track_sample(6, 0.5f));

    reset();
    crumb_sound_play(1, 1.0f);
    for (int i = 0; i < 1000; ++i) crumb_sound_seek(0.0f);
    crumb_sound_stop();
    assert_silent(16);
    assert(crumb_sound_position() == 0.0f);

    reset();
    crumb_sound_play(1, 1.0f);
    crumb_audio_render(samples, 1);
    for (int i = 0; i < 1000; ++i) crumb_sound_seek(0.0f);
    crumb_sound_pause();
    assert_silent(16);
    crumb_sound_resume();
    crumb_audio_render(samples, 1);
    assert(samples[0] == track_sample(0, 1.0f));

    crumb_sound_stop();
    crumb_sound_play(99, 1.0f);
    crumb_sound_play(1, NAN);
    assert_silent(8);
}

static void *play_while_publish_is_held(void *unused) {
    (void)unused;
    crumb_sound_play(1, 1.0f);
    return NULL;
}

static void play_state_precedes_command_publication(void) {
    pthread_t producer;

    reset();
    atomic_store(&sound_publish_waiting, 0);
    atomic_store(&hold_sound_publish, 1);
    assert(pthread_create(&producer, NULL, play_while_publish_is_held, NULL) == 0);
    while (!atomic_load_explicit(&sound_publish_waiting, memory_order_acquire)) {}

    /* The play state is ready, but the callback cannot consume the unpublished slot. */
    assert_silent(4);
    atomic_store_explicit(&hold_sound_publish, 0, memory_order_release);
    assert(pthread_join(producer, NULL) == 0);

    crumb_audio_render(samples, 2);
    assert(samples[0] == track_sample(0, 1.0f));
    assert(samples[1] == track_sample(1, 1.0f));
}

int main(void) {
    tone_duration_frequency_and_envelope();
    validation_and_clamps();
    deterministic_noise_and_chunk_boundaries();
    bounded_overload_and_reset();
    concurrent_submission();
    pcm_track_transport_and_saturation();
    play_state_precedes_command_publication();
    return 0;
}
