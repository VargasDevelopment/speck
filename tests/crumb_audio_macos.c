/* Exercise the actual adapter against a deterministic failing audio driver. */
#define AudioQueueNewOutput test_new_output
#define AudioQueueAllocateBuffer test_allocate
#define AudioQueueEnqueueBuffer test_enqueue
#define AudioQueueStart test_start
#define AudioQueueDispose test_dispose
#include "../runtime/crumb/audio_macos.c"
#include "crumb.h"

#include <assert.h>
#include <string.h>

static AudioQueueOutputCallback callback;
static AudioQueueBuffer buffers[3];
static int16_t pcm[3][256];
static unsigned int allocated;
static unsigned int enqueued;
static unsigned int disposals;
static int fail_at;
static int operation;
static int live;

static OSStatus step(void) {
    return ++operation == fail_at ? -1 : noErr;
}

OSStatus test_new_output(const AudioStreamBasicDescription *format,
                        AudioQueueOutputCallback output, void *context,
                        CFRunLoopRef loop, CFStringRef mode, UInt32 flags,
                        AudioQueueRef *queue) {
    assert(!live && context == NULL && loop == NULL && mode == NULL && flags == 0);
    assert(format->mSampleRate == 48000 && format->mChannelsPerFrame == 1);
    assert(format->mBitsPerChannel == 16 && format->mBytesPerFrame == 2);
    if (step() != noErr) return -1;
    callback = output;
    allocated = enqueued = 0;
    live = 1;
    *queue = (AudioQueueRef)(uintptr_t)1;
    return noErr;
}

OSStatus test_allocate(AudioQueueRef queue, UInt32 size, AudioQueueBufferRef *buffer) {
    assert(live && queue != NULL && allocated < 3 && size == sizeof pcm[0]);
    if (step() != noErr) return -1;
    /* mAudioData and mAudioDataBytesCapacity are const SDK members. */
    const AudioQueueBuffer initial = {.mAudioDataBytesCapacity = sizeof pcm[0], .mAudioData = pcm[allocated]};
    memcpy(&buffers[allocated], &initial, sizeof initial);
    *buffer = &buffers[allocated++];
    return noErr;
}

OSStatus test_enqueue(AudioQueueRef queue, AudioQueueBufferRef buffer,
                      UInt32 count, const AudioStreamPacketDescription *descriptions) {
    assert(live && queue != NULL && count == 0 && descriptions == NULL);
    assert(buffer->mAudioDataByteSize == sizeof pcm[0]);
    if (step() != noErr) return -1;
    ++enqueued;
    return noErr;
}

OSStatus test_start(AudioQueueRef queue, const AudioTimeStamp *start) {
    assert(live && queue != NULL && start == NULL && enqueued == 3);
    return step();
}

OSStatus test_dispose(AudioQueueRef queue, Boolean immediate) {
    const unsigned int previous_enqueues = enqueued;
    assert(live && queue != NULL && immediate);
    /* A callback already arriving during disposal must not render or enqueue. */
    if (allocated != 0) callback(NULL, queue, &buffers[0]);
    assert(previous_enqueues == enqueued);
    live = 0;
    ++disposals;
    return noErr;
}

int main(void) {
    /* Fail queue creation, each of three allocations/enqueues, then start. */
    for (fail_at = 1; fail_at <= 8; ++fail_at) {
        const unsigned int previous_disposals = disposals;
        operation = 0;
        crumb_audio_init();
        assert(!live);
        assert(disposals == previous_disposals + (fail_at != 1));
        crumb_tone(880.0f, 0.1f, 1.0f);
        int16_t silence[256];
        crumb_audio_render(silence, 256);
        for (int i = 0; i < 256; ++i) assert(silence[i] == 0);
        crumb_audio_shutdown();
        assert(disposals == previous_disposals + (fail_at != 1));
    }
    fail_at = 0;
    operation = 0;
    crumb_audio_init();
    assert(live);
    crumb_tone(880.0f, 0.1f, 1.0f);
    callback(NULL, output_queue, &buffers[0]);
    assert(pcm[0][100] != 0);
    /* A driver enqueue failure disables effects without callback logging. */
    fail_at = operation + 1;
    callback(NULL, output_queue, &buffers[0]);
    crumb_audio_render(pcm[0], 256);
    for (int i = 0; i < 256; ++i) assert(pcm[0][i] == 0);
    crumb_audio_shutdown();
    assert(!live);
    crumb_audio_shutdown();
    return 0;
}
