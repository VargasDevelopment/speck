#include "crumb_internal.h"
#include "audio_internal.h"

#include <AudioToolbox/AudioQueue.h>
#include <stdatomic.h>
#include <stdio.h>

enum { OUTPUT_BUFFERS = 3, BUFFER_SAMPLES = 256 };
static AudioQueueRef output_queue;
static atomic_int stopping;

static void output_callback(void *context, AudioQueueRef queue, AudioQueueBufferRef buffer) {
    (void)context;
    if (atomic_load_explicit(&stopping, memory_order_acquire)) {
        return;
    }
    crumb_audio_render(buffer->mAudioData, BUFFER_SAMPLES);
    buffer->mAudioDataByteSize = BUFFER_SAMPLES * sizeof(int16_t);
    if (AudioQueueEnqueueBuffer(queue, buffer, 0, NULL) != noErr) {
        /* The real-time callback does no logging or device teardown. */
        crumb_audio_enable(0);
    }
}

void crumb_audio_shutdown(void) {
    crumb_audio_enable(0);
    atomic_store_explicit(&stopping, 1, memory_order_release);
    if (output_queue != NULL) {
        /* Outside callbacks, synchronous disposal guarantees no callbacks after return. */
        AudioQueueDispose(output_queue, true);
        output_queue = NULL;
    }
    crumb_audio_reset();
}

void crumb_audio_init(void) {
    const AudioStreamBasicDescription format = {
        .mSampleRate = CRUMB_AUDIO_SAMPLE_RATE,
        .mFormatID = kAudioFormatLinearPCM,
        .mFormatFlags = kAudioFormatFlagIsSignedInteger | kAudioFormatFlagIsPacked,
        .mBytesPerPacket = sizeof(int16_t),
        .mFramesPerPacket = 1,
        .mBytesPerFrame = sizeof(int16_t),
        .mChannelsPerFrame = 1,
        .mBitsPerChannel = 16
    };

    crumb_audio_shutdown();
    atomic_store_explicit(&stopping, 0, memory_order_release);
    if (AudioQueueNewOutput(&format, output_callback, NULL, NULL, NULL, 0, &output_queue) != noErr) {
        goto failed;
    }
    for (unsigned int i = 0; i < OUTPUT_BUFFERS; ++i) {
        AudioQueueBufferRef buffer;
        if (AudioQueueAllocateBuffer(output_queue, BUFFER_SAMPLES * sizeof(int16_t), &buffer) != noErr) {
            goto failed;
        }
        crumb_audio_render(buffer->mAudioData, BUFFER_SAMPLES);
        buffer->mAudioDataByteSize = BUFFER_SAMPLES * sizeof(int16_t);
        if (AudioQueueEnqueueBuffer(output_queue, buffer, 0, NULL) != noErr) {
            goto failed;
        }
    }
    crumb_audio_enable(1);
    if (AudioQueueStart(output_queue, NULL) != noErr) {
        goto failed;
    }
    return;

failed:
    crumb_audio_shutdown();
    fputs("CRuMB could not initialize audio; continuing silently\n", stderr);
}
