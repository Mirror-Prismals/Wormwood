// Control-thread setup only. Use CoreAudio's supported aggregate API instead
// of JACK2's obsolete CoreAudio plug-in lookup for separated input/output.
#import <Foundation/Foundation.h>
#import <CoreAudio/CoreAudio.h>
#include <signal.h>
#include <errno.h>
#include <limits.h>

static OSStatus deviceForUID(NSString *uid, AudioDeviceID *device) {
    CFStringRef value = (__bridge CFStringRef)uid;
    AudioObjectPropertyAddress address = {kAudioHardwarePropertyTranslateUIDToDevice,
        kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMain};
    UInt32 size = sizeof(*device);
    OSStatus result = AudioObjectGetPropertyData(kAudioObjectSystemObject, &address,
        sizeof(value), &value, &size, device);
    return result ? result : (*device ? noErr : kAudioHardwareBadDeviceError);
}
static OSStatus channels(AudioDeviceID device, AudioObjectPropertyScope scope, UInt32 *count) {
    AudioObjectPropertyAddress address = {kAudioDevicePropertyStreamConfiguration,
        scope, kAudioObjectPropertyElementMain};
    UInt32 size = 0;
    OSStatus result = AudioObjectGetPropertyDataSize(device, &address, 0, NULL, &size);
    if (result) return result;
    if (size < sizeof(UInt32)) return kAudioHardwareBadPropertySizeError;
    AudioBufferList *buffers = calloc(1, size);
    if (!buffers) return kAudioHardwareUnspecifiedError;
    result = AudioObjectGetPropertyData(device, &address, 0, NULL, &size, buffers);
    *count = 0;
    if (!result) for (UInt32 i = 0; i < buffers->mNumberBuffers; ++i)
        *count += buffers->mBuffers[i].mNumberChannels;
    free(buffers);
    return result;
}
// Published aggregates survive a crashed owner. Reclaim only our generated
// aggregate UIDs whose process is demonstrably gone; never touch live sessions
// or other applications' / user-created audio devices.
static void removeAbandonedSessionDevices(void) {
    AudioObjectPropertyAddress address = {kAudioHardwarePropertyDevices,
        kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMain};
    UInt32 bytes = 0;
    if (AudioObjectGetPropertyDataSize(kAudioObjectSystemObject, &address, 0, NULL, &bytes)) return;
    AudioDeviceID *devices = malloc(bytes);
    if (!devices) return;
    if (!AudioObjectGetPropertyData(kAudioObjectSystemObject, &address, 0, NULL, &bytes, devices)) {
        for (UInt32 i = 0; i < bytes / sizeof(*devices); ++i) {
            AudioObjectPropertyAddress property = {kAudioObjectPropertyClass,
                kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMain};
            AudioClassID kind = 0; UInt32 size = sizeof(kind);
            if (AudioObjectGetPropertyData(devices[i], &property, 0, NULL, &size, &kind)
                || kind != kAudioAggregateDeviceClassID) continue;
            property.mSelector = kAudioDevicePropertyDeviceUID;
            CFStringRef value = NULL; size = sizeof(value);
            if (AudioObjectGetPropertyData(devices[i], &property, 0, NULL, &size, &value) || !value) continue;
            NSString *uid = CFBridgingRelease(value);
            NSArray<NSString *> *parts = [uid componentsSeparatedByString:@"."];
            if (![uid hasPrefix:@"org.wormwood.duplex."] || parts.count != 6) continue;
            NSCharacterSet *invalid = [[NSCharacterSet characterSetWithCharactersInString:@"0123456789"] invertedSet];
            BOOL valid = YES;
            for (NSUInteger part = 3; part < 6; ++part)
                if (!parts[part].length || [parts[part] rangeOfCharacterFromSet:invalid].location != NSNotFound) valid = NO;
            long long owner = parts[3].longLongValue;
            if (valid && owner > 1 && owner <= INT_MAX && kill((pid_t)owner, 0) == -1 && errno == ESRCH)
                AudioHardwareDestroyAggregateDevice(devices[i]);
        }
    }
    free(devices);
}
int32_t ww_mac_audio_aggregate_create(const char *input, const char *output,
    const char *uid, AudioDeviceID *aggregate, UInt32 *inputStart, UInt32 *inputCount) {
    @autoreleasepool {
        *aggregate = 0;
        removeAbandonedSessionDevices();
        NSString *inputUID = [NSString stringWithUTF8String:input];
        NSString *outputUID = [NSString stringWithUTF8String:output];
        NSString *aggregateUID = [NSString stringWithUTF8String:uid];
        if (!inputUID || !outputUID || !aggregateUID) return kAudioHardwareIllegalOperationError;
        AudioDeviceID inputDevice = 0, outputDevice = 0;
        OSStatus result = deviceForUID(inputUID, &inputDevice);
        if (!result) result = deviceForUID(outputUID, &outputDevice);
        // Output is first so JACK output channels 0/1 always reach the selected
        // speakers. Skip any inputs on that device when mapping the chosen mic.
        if (!result) result = channels(outputDevice, kAudioObjectPropertyScopeInput, inputStart);
        if (!result) result = channels(inputDevice, kAudioObjectPropertyScopeInput, inputCount);
        if (result) return result;
        if (!*inputCount) return kAudioHardwareBadDeviceError;
        NSDictionary *description = @{
            @kAudioAggregateDeviceUIDKey: aggregateUID,
            @kAudioAggregateDeviceNameKey: @"Mirror session audio",
            // JACK is another process. Never change either OS default device.
            // Session owns and destroys this published device after JACK stops.
            @kAudioAggregateDeviceIsPrivateKey: @NO,
            @kAudioAggregateDeviceMainSubDeviceKey: outputUID,
            @kAudioAggregateDeviceSubDeviceListKey: @[
                @{@kAudioSubDeviceUIDKey: outputUID},
                @{@kAudioSubDeviceUIDKey: inputUID, @kAudioSubDeviceDriftCompensationKey: @YES}
            ]
        };
        return AudioHardwareCreateAggregateDevice((__bridge CFDictionaryRef)description, aggregate);
    }
}
int32_t ww_mac_audio_aggregate_destroy(AudioDeviceID aggregate) {
    return AudioHardwareDestroyAggregateDevice(aggregate);
}
