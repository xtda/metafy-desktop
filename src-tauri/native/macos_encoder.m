#import <AVFoundation/AVFoundation.h>
#import <CoreAudio/CoreAudioTypes.h>
#import <CoreMedia/CoreMedia.h>
#import <CoreVideo/CoreVideo.h>
#import <Foundation/Foundation.h>
#import <dispatch/dispatch.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

static char *MetafyCopyCString(NSString *message) {
    const char *utf8 = message.UTF8String;
    if (utf8 == NULL) {
        utf8 = "Native macOS encoder failed.";
    }

    size_t length = strlen(utf8);
    char *copy = (char *)malloc(length + 1);
    if (copy == NULL) {
        return NULL;
    }
    memcpy(copy, utf8, length + 1);
    return copy;
}

static void MetafySetError(char **error_out, NSString *format, ...) {
    if (error_out == NULL) {
        return;
    }

    va_list args;
    va_start(args, format);
    NSString *message = [[NSString alloc] initWithFormat:format arguments:args];
    va_end(args);

    *error_out = MetafyCopyCString(message);
}

static void MetafySetNSError(char **error_out, NSString *prefix, NSError *error) {
    if (error == nil) {
        MetafySetError(error_out, @"%@.", prefix);
        return;
    }
    MetafySetError(error_out, @"%@: %@", prefix, error.localizedDescription);
}

static NSString *MetafyStringFromCString(const char *value) {
    if (value == NULL || value[0] == '\0') {
        return nil;
    }
    return [NSString stringWithUTF8String:value];
}

static BOOL MetafyWaitForReady(AVAssetWriter *writer, AVAssetWriterInput *input, NSString *label, char **error_out) {
    NSDate *deadline = [NSDate dateWithTimeIntervalSinceNow:30.0];
    while (!input.readyForMoreMediaData) {
        if (writer.status == AVAssetWriterStatusFailed) {
            MetafySetNSError(error_out, [NSString stringWithFormat:@"AVAssetWriter failed while waiting for %@", label], writer.error);
            return NO;
        }
        if (writer.status == AVAssetWriterStatusCancelled) {
            MetafySetError(error_out, @"AVAssetWriter was cancelled while waiting for %@.", label);
            return NO;
        }
        if (deadline.timeIntervalSinceNow <= 0.0) {
            MetafySetError(error_out, @"Timed out waiting for AVAssetWriter input readiness (%@).", label);
            return NO;
        }
        [NSThread sleepForTimeInterval:0.001];
    }
    return YES;
}

static BOOL MetafyCopyBGRAFrameToPixelBuffer(NSData *frame_data, CVPixelBufferRef pixel_buffer, int64_t width, int64_t height) {
    CVPixelBufferLockBaseAddress(pixel_buffer, 0);
    uint8_t *destination = (uint8_t *)CVPixelBufferGetBaseAddress(pixel_buffer);
    size_t destination_bytes_per_row = CVPixelBufferGetBytesPerRow(pixel_buffer);
    const uint8_t *source = (const uint8_t *)frame_data.bytes;
    size_t source_bytes_per_row = (size_t)width * 4;

    if (destination_bytes_per_row == source_bytes_per_row) {
        memcpy(destination, source, source_bytes_per_row * (size_t)height);
    } else {
        for (int64_t row = 0; row < height; row++) {
            memcpy(
                destination + ((size_t)row * destination_bytes_per_row),
                source + ((size_t)row * source_bytes_per_row),
                source_bytes_per_row
            );
        }
    }

    CVPixelBufferUnlockBaseAddress(pixel_buffer, 0);
    return YES;
}

static BOOL MetafyCreateBGRAFramePixelBuffer(
    AVAssetWriterInputPixelBufferAdaptor *pixel_buffer_adaptor,
    NSDictionary *pixel_buffer_attributes,
    int64_t width,
    int64_t height,
    CVPixelBufferRef *pixel_buffer_out,
    char **error_out
) {
    *pixel_buffer_out = NULL;
    CVPixelBufferPoolRef pixel_buffer_pool = pixel_buffer_adaptor.pixelBufferPool;
    CVReturn pool_status = kCVReturnInvalidPixelBufferAttributes;
    if (pixel_buffer_pool != NULL) {
        pool_status = CVPixelBufferPoolCreatePixelBuffer(
            kCFAllocatorDefault,
            pixel_buffer_pool,
            pixel_buffer_out
        );
        if (pool_status == kCVReturnSuccess && *pixel_buffer_out != NULL) {
            return YES;
        }
    }

    CVReturn direct_status = CVPixelBufferCreate(
        kCFAllocatorDefault,
        (size_t)width,
        (size_t)height,
        kCVPixelFormatType_32BGRA,
        (__bridge CFDictionaryRef)pixel_buffer_attributes,
        pixel_buffer_out
    );
    if (direct_status == kCVReturnSuccess && *pixel_buffer_out != NULL) {
        return YES;
    }

    if (pixel_buffer_pool == NULL) {
        MetafySetError(
            error_out,
            @"AVFoundation did not create a pixel buffer pool, and direct CVPixelBuffer allocation failed (%d).",
            direct_status
        );
    } else {
        MetafySetError(
            error_out,
            @"Unable to allocate CVPixelBuffer (pool %d, direct %d).",
            pool_status,
            direct_status
        );
    }
    return NO;
}

static BOOL MetafyAppendVideoFrame(
    AVAssetWriter *writer,
    AVAssetWriterInput *video_input,
    AVAssetWriterInputPixelBufferAdaptor *pixel_buffer_adaptor,
    NSDictionary *pixel_buffer_attributes,
    NSFileHandle *video_file,
    int64_t width,
    int64_t height,
    int64_t frame_rate,
    int64_t frame_count,
    NSUInteger frame_byte_count,
    int64_t frame_index,
    char **error_out
) {
    if (frame_index >= frame_count) {
        [video_input markAsFinished];
        return YES;
    }

    @autoreleasepool {
        NSData *frame_data = [video_file readDataOfLength:frame_byte_count];
        if (frame_data.length != frame_byte_count) {
            MetafySetError(
                error_out,
                @"Raw video ended early at frame %lld; expected %llu bytes, read %llu bytes.",
                (long long)frame_index,
                (unsigned long long)frame_byte_count,
                (unsigned long long)frame_data.length
            );
            return NO;
        }

        if (!MetafyWaitForReady(writer, video_input, @"raw video frame", error_out)) {
            return NO;
        }

        CVPixelBufferRef pixel_buffer = NULL;
        if (!MetafyCreateBGRAFramePixelBuffer(
                pixel_buffer_adaptor,
                pixel_buffer_attributes,
                width,
                height,
                &pixel_buffer,
                error_out
            )) {
            return NO;
        }

        MetafyCopyBGRAFrameToPixelBuffer(frame_data, pixel_buffer, width, height);
        CMTime presentation_time = CMTimeMake(frame_index, (int32_t)frame_rate);
        BOOL appended = [pixel_buffer_adaptor appendPixelBuffer:pixel_buffer withPresentationTime:presentation_time];
        CVPixelBufferRelease(pixel_buffer);

        if (!appended) {
            MetafySetNSError(error_out, @"Unable to append video frame to AVAssetWriter", writer.error);
            return NO;
        }
    }

    return YES;
}

static CMAudioFormatDescriptionRef MetafyCreateAudioFormatDescription(
    int64_t sample_rate,
    int64_t channels,
    char **error_out
) {
    AudioStreamBasicDescription audio_description;
    memset(&audio_description, 0, sizeof(audio_description));
    audio_description.mSampleRate = (Float64)sample_rate;
    audio_description.mFormatID = kAudioFormatLinearPCM;
    audio_description.mFormatFlags = kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked | kAudioFormatFlagsNativeEndian;
    audio_description.mBytesPerPacket = (UInt32)(channels * 4);
    audio_description.mFramesPerPacket = 1;
    audio_description.mBytesPerFrame = (UInt32)(channels * 4);
    audio_description.mChannelsPerFrame = (UInt32)channels;
    audio_description.mBitsPerChannel = 32;

    CMAudioFormatDescriptionRef format_description = NULL;
    OSStatus status = CMAudioFormatDescriptionCreate(
        kCFAllocatorDefault,
        &audio_description,
        0,
        NULL,
        0,
        NULL,
        NULL,
        &format_description
    );
    if (status != noErr || format_description == NULL) {
        MetafySetError(error_out, @"Unable to create audio format description (%d).", (int)status);
        return NULL;
    }
    return format_description;
}

static BOOL MetafyAppendAudioChunk(
    AVAssetWriter *writer,
    AVAssetWriterInput *audio_input,
    NSFileHandle *audio_file,
    CMAudioFormatDescriptionRef audio_format_description,
    int64_t sample_rate,
    int64_t channels,
    int64_t max_audio_frames,
    int64_t *presentation_frame,
    BOOL *audio_finished,
    char **error_out
) {
    const NSUInteger bytes_per_frame = (NSUInteger)channels * sizeof(float);
    NSUInteger frames_per_chunk = 4096;
    if (max_audio_frames > 0) {
        if (*presentation_frame >= max_audio_frames) {
            [audio_input markAsFinished];
            *audio_finished = YES;
            return YES;
        }
        int64_t remaining_frames = max_audio_frames - *presentation_frame;
        if (remaining_frames < (int64_t)frames_per_chunk) {
            frames_per_chunk = (NSUInteger)remaining_frames;
        }
    }
    const NSUInteger bytes_per_chunk = bytes_per_frame * frames_per_chunk;

    @autoreleasepool {
        NSData *audio_data = [audio_file readDataOfLength:bytes_per_chunk];
        if (audio_data.length == 0) {
            [audio_input markAsFinished];
            *audio_finished = YES;
            return YES;
        }
        if (audio_data.length % bytes_per_frame != 0) {
            MetafySetError(error_out, @"Prepared audio length is not aligned to full f32 stereo frames.");
            return NO;
        }

        if (!MetafyWaitForReady(writer, audio_input, @"prepared audio chunk", error_out)) {
            return NO;
        }

        CMBlockBufferRef block_buffer = NULL;
        OSStatus block_status = CMBlockBufferCreateWithMemoryBlock(
            kCFAllocatorDefault,
            NULL,
            audio_data.length,
            kCFAllocatorDefault,
            NULL,
            0,
            audio_data.length,
            0,
            &block_buffer
        );
        if (block_status != kCMBlockBufferNoErr || block_buffer == NULL) {
            MetafySetError(error_out, @"Unable to create audio block buffer (%d).", (int)block_status);
            return NO;
        }

        block_status = CMBlockBufferReplaceDataBytes(audio_data.bytes, block_buffer, 0, audio_data.length);
        if (block_status != kCMBlockBufferNoErr) {
            CFRelease(block_buffer);
            MetafySetError(error_out, @"Unable to fill audio block buffer (%d).", (int)block_status);
            return NO;
        }

        CMItemCount sample_count = (CMItemCount)(audio_data.length / bytes_per_frame);
        CMSampleTimingInfo timing;
        timing.duration = CMTimeMake(1, (int32_t)sample_rate);
        timing.presentationTimeStamp = CMTimeMake(*presentation_frame, (int32_t)sample_rate);
        timing.decodeTimeStamp = kCMTimeInvalid;

        size_t sample_size = bytes_per_frame;
        CMSampleBufferRef sample_buffer = NULL;
        OSStatus sample_status = CMSampleBufferCreateReady(
            kCFAllocatorDefault,
            block_buffer,
            audio_format_description,
            sample_count,
            1,
            &timing,
            1,
            &sample_size,
            &sample_buffer
        );
        CFRelease(block_buffer);
        if (sample_status != noErr || sample_buffer == NULL) {
            MetafySetError(error_out, @"Unable to create audio sample buffer (%d).", (int)sample_status);
            return NO;
        }

        BOOL appended = [audio_input appendSampleBuffer:sample_buffer];
        CFRelease(sample_buffer);
        if (!appended) {
            MetafySetNSError(error_out, @"Unable to append audio samples to AVAssetWriter", writer.error);
            return NO;
        }

        *presentation_frame += (int64_t)sample_count;
    }

    return YES;
}

static BOOL MetafyShouldAppendVideoNext(
    int64_t frame_index,
    int64_t frame_rate,
    int64_t audio_presentation_frame,
    int64_t audio_sample_rate
) {
    return (frame_index * audio_sample_rate) <= (audio_presentation_frame * frame_rate);
}

static BOOL MetafyAppendMedia(
    AVAssetWriter *writer,
    AVAssetWriterInput *video_input,
    AVAssetWriterInputPixelBufferAdaptor *pixel_buffer_adaptor,
    NSDictionary *pixel_buffer_attributes,
    NSFileHandle *video_file,
    int64_t width,
    int64_t height,
    int64_t frame_rate,
    int64_t frame_count,
    NSUInteger frame_byte_count,
    AVAssetWriterInput *audio_input,
    NSFileHandle *audio_file,
    CMAudioFormatDescriptionRef audio_format_description,
    int64_t audio_sample_rate,
    int64_t audio_channels,
    char **error_out
) {
    int64_t frame_index = 0;
    int64_t audio_presentation_frame = 0;
    BOOL video_finished = NO;
    BOOL audio_finished = audio_input == nil;

    while (!video_finished || !audio_finished) {
        BOOL append_video_next = !video_finished
            && (audio_finished
                || MetafyShouldAppendVideoNext(
                    frame_index,
                    frame_rate,
                    audio_presentation_frame,
                    audio_sample_rate
                ));

        if (append_video_next) {
            if (!MetafyAppendVideoFrame(
                    writer,
                    video_input,
                    pixel_buffer_adaptor,
                    pixel_buffer_attributes,
                    video_file,
                    width,
                    height,
                    frame_rate,
                    frame_count,
                    frame_byte_count,
                    frame_index,
                    error_out
                )) {
                return NO;
            }
            frame_index += 1;
            if (frame_index >= frame_count) {
                [video_input markAsFinished];
                video_finished = YES;
            }
        } else if (!audio_finished) {
            if (!MetafyAppendAudioChunk(
                    writer,
                    audio_input,
                    audio_file,
                    audio_format_description,
                    audio_sample_rate,
                    audio_channels,
                    0,
                    &audio_presentation_frame,
                    &audio_finished,
                    error_out
                )) {
                return NO;
            }
        }
    }

    return YES;
}

static int64_t MetafyDurationMsFromFrames(int64_t frame_count, int64_t frame_rate) {
    if (frame_count <= 0 || frame_rate <= 0) {
        return 0;
    }
    return (frame_count * 1000 + (frame_rate / 2)) / frame_rate;
}

@interface MetafySegmentedRecorder : NSObject
@property(nonatomic, copy) NSString *manifestPath;
@property(nonatomic, copy) NSString *manifestDirectory;
@property(nonatomic, copy) NSString *chunkBaseName;
@property(nonatomic) int64_t width;
@property(nonatomic) int64_t height;
@property(nonatomic) int64_t frameRate;
@property(nonatomic) int64_t maxFramesPerChunk;
@property(nonatomic) int64_t totalFrameCount;
@property(nonatomic) int64_t currentChunkIndex;
@property(nonatomic) int64_t currentChunkStartFrame;
@property(nonatomic) int64_t currentChunkFrameCount;
@property(nonatomic, strong) NSMutableArray<NSDictionary *> *chunks;
@property(nonatomic, strong) AVAssetWriter *writer;
@property(nonatomic, strong) AVAssetWriterInput *videoInput;
@property(nonatomic, strong) AVAssetWriterInputPixelBufferAdaptor *pixelBufferAdaptor;
@property(nonatomic, strong) NSDictionary *pixelBufferAttributes;
@property(nonatomic, copy) NSString *currentChunkFileName;
- (instancetype)initWithManifestPath:(NSString *)manifestPath
                               width:(int64_t)width
                              height:(int64_t)height
                           frameRate:(int64_t)frameRate
                   maxFramesPerChunk:(int64_t)maxFramesPerChunk
                           errorOut:(char **)error_out;
- (BOOL)appendFrameBytes:(const uint8_t *)bytes
               byteCount:(NSUInteger)byteCount
               elapsedMs:(int64_t)elapsedMs
           displayTimeMs:(int64_t)displayTimeMs
                errorOut:(char **)error_out;
- (BOOL)finishWithErrorOut:(char **)error_out;
- (void)cancelOpenChunk;
@end

@implementation MetafySegmentedRecorder

- (instancetype)initWithManifestPath:(NSString *)manifestPath
                               width:(int64_t)width
                              height:(int64_t)height
                           frameRate:(int64_t)frameRate
                   maxFramesPerChunk:(int64_t)maxFramesPerChunk
                           errorOut:(char **)error_out {
    self = [super init];
    if (self == nil) {
        return nil;
    }
    if (manifestPath.length == 0 || width <= 0 || height <= 0 || frameRate <= 0 || maxFramesPerChunk <= 0) {
        MetafySetError(error_out, @"Chunked recorder received invalid manifest path or video settings.");
        return nil;
    }
    if (width > INT64_MAX / height || width * height > INT64_MAX / 4) {
        MetafySetError(error_out, @"Chunked recorder video dimensions are too large.");
        return nil;
    }

    _manifestPath = [manifestPath copy];
    _manifestDirectory = [[manifestPath stringByDeletingLastPathComponent] copy];
    if (_manifestDirectory.length == 0) {
        _manifestDirectory = @"." ;
    }
    _chunkBaseName = [[manifestPath.lastPathComponent stringByDeletingPathExtension] copy];
    _width = width;
    _height = height;
    _frameRate = frameRate;
    _maxFramesPerChunk = maxFramesPerChunk;
    _chunks = [NSMutableArray array];
    _pixelBufferAttributes = @{
        (NSString *)kCVPixelBufferPixelFormatTypeKey: @(kCVPixelFormatType_32BGRA),
        (NSString *)kCVPixelBufferWidthKey: @(width),
        (NSString *)kCVPixelBufferHeightKey: @(height),
        (NSString *)kCVPixelBufferIOSurfacePropertiesKey: @{}
    };

    NSError *directory_error = nil;
    if (![[NSFileManager defaultManager] createDirectoryAtPath:_manifestDirectory
                                   withIntermediateDirectories:YES
                                                    attributes:nil
                                                         error:&directory_error]) {
        MetafySetNSError(error_out, @"Unable to create chunked video directory", directory_error);
        return nil;
    }
    [[NSFileManager defaultManager] removeItemAtPath:_manifestPath error:nil];

    return self;
}

- (NSString *)chunkFileNameForIndex:(int64_t)index {
    return [NSString stringWithFormat:@"%@-%05lld.mp4", self.chunkBaseName, (long long)index];
}

- (BOOL)writeManifestWithStatus:(NSString *)status errorOut:(char **)error_out {
    NSDictionary *manifest = @{
        @"format": @"metafy_chunked_h264_segments_v1",
        @"status": status,
        @"codec": @"h264",
        @"container": @"mp4",
        @"width": @(self.width),
        @"height": @(self.height),
        @"frameRate": @(self.frameRate),
        @"frameCount": @(self.totalFrameCount),
        @"durationMs": @(MetafyDurationMsFromFrames(self.totalFrameCount, self.frameRate)),
        @"thumbnailFramePath": [NSString stringWithFormat:@"%@-thumbnail.bgra", self.chunkBaseName],
        @"chunks": self.chunks
    };
    NSError *json_error = nil;
    NSData *json_data = [NSJSONSerialization dataWithJSONObject:manifest options:NSJSONWritingPrettyPrinted error:&json_error];
    if (json_data == nil) {
        MetafySetNSError(error_out, @"Unable to serialize chunked video manifest", json_error);
        return NO;
    }
    if (![json_data writeToFile:self.manifestPath options:NSDataWritingAtomic error:&json_error]) {
        MetafySetNSError(error_out, @"Unable to write chunked video manifest", json_error);
        return NO;
    }
    return YES;
}

- (BOOL)startChunkWithErrorOut:(char **)error_out {
    self.currentChunkFileName = [self chunkFileNameForIndex:self.currentChunkIndex];
    NSString *chunk_path = [self.manifestDirectory stringByAppendingPathComponent:self.currentChunkFileName];
    NSURL *chunk_url = [NSURL fileURLWithPath:chunk_path];
    [[NSFileManager defaultManager] removeItemAtURL:chunk_url error:nil];

    NSError *writer_error = nil;
    self.writer = [[AVAssetWriter alloc] initWithURL:chunk_url fileType:AVFileTypeMPEG4 error:&writer_error];
    if (self.writer == nil) {
        MetafySetNSError(error_out, @"Unable to create chunked AVAssetWriter", writer_error);
        return NO;
    }
    self.writer.shouldOptimizeForNetworkUse = YES;

    NSDictionary *video_settings = @{
        AVVideoCodecKey: AVVideoCodecTypeH264,
        AVVideoWidthKey: @(self.width),
        AVVideoHeightKey: @(self.height),
        AVVideoCompressionPropertiesKey: @{
            AVVideoAverageBitRateKey: @((NSInteger)MAX(1, self.width * self.height * self.frameRate / 2)),
            AVVideoExpectedSourceFrameRateKey: @(self.frameRate),
            AVVideoMaxKeyFrameIntervalKey: @(self.frameRate * 2),
            AVVideoProfileLevelKey: AVVideoProfileLevelH264HighAutoLevel
        }
    };
    self.videoInput = [AVAssetWriterInput assetWriterInputWithMediaType:AVMediaTypeVideo outputSettings:video_settings];
    self.videoInput.expectsMediaDataInRealTime = YES;
    if (![self.writer canAddInput:self.videoInput]) {
        MetafySetError(error_out, @"Chunked AVAssetWriter cannot add the H.264 video input.");
        return NO;
    }
    [self.writer addInput:self.videoInput];
    self.pixelBufferAdaptor =
        [AVAssetWriterInputPixelBufferAdaptor assetWriterInputPixelBufferAdaptorWithAssetWriterInput:self.videoInput
                                                                         sourcePixelBufferAttributes:self.pixelBufferAttributes];

    if (![self.writer startWriting]) {
        MetafySetNSError(error_out, @"Chunked AVAssetWriter could not start writing", self.writer.error);
        return NO;
    }
    [self.writer startSessionAtSourceTime:kCMTimeZero];

    self.currentChunkStartFrame = self.totalFrameCount;
    self.currentChunkFrameCount = 0;
    return YES;
}

- (BOOL)finishCurrentChunkWithErrorOut:(char **)error_out {
    if (self.writer == nil) {
        return YES;
    }
    if (self.currentChunkFrameCount <= 0) {
        [self.writer cancelWriting];
        self.writer = nil;
        self.videoInput = nil;
        self.pixelBufferAdaptor = nil;
        self.currentChunkFileName = nil;
        return YES;
    }

    [self.videoInput markAsFinished];
    dispatch_semaphore_t finish_semaphore = dispatch_semaphore_create(0);
    [self.writer finishWritingWithCompletionHandler:^{
        dispatch_semaphore_signal(finish_semaphore);
    }];
    dispatch_semaphore_wait(finish_semaphore, DISPATCH_TIME_FOREVER);
    if (self.writer.status != AVAssetWriterStatusCompleted) {
        MetafySetNSError(error_out, @"Chunked AVAssetWriter did not complete", self.writer.error);
        return NO;
    }

    [self.chunks addObject:@{
        @"path": self.currentChunkFileName,
        @"index": @(self.currentChunkIndex),
        @"startFrame": @(self.currentChunkStartFrame),
        @"frameCount": @(self.currentChunkFrameCount),
        @"durationMs": @(MetafyDurationMsFromFrames(self.currentChunkFrameCount, self.frameRate))
    }];
    self.currentChunkIndex += 1;
    self.writer = nil;
    self.videoInput = nil;
    self.pixelBufferAdaptor = nil;
    self.currentChunkFileName = nil;
    self.currentChunkFrameCount = 0;

    return [self writeManifestWithStatus:@"recording" errorOut:error_out];
}

- (BOOL)appendFrameBytes:(const uint8_t *)bytes
               byteCount:(NSUInteger)byteCount
               elapsedMs:(int64_t)elapsedMs
           displayTimeMs:(int64_t)displayTimeMs
                errorOut:(char **)error_out {
    (void)elapsedMs;
    (void)displayTimeMs;
    NSUInteger expected_byte_count = (NSUInteger)(self.width * self.height * 4);
    if (bytes == NULL || byteCount != expected_byte_count) {
        MetafySetError(
            error_out,
            @"Chunked recorder received %llu BGRA bytes; expected %llu.",
            (unsigned long long)byteCount,
            (unsigned long long)expected_byte_count
        );
        return NO;
    }
    if (self.writer == nil && ![self startChunkWithErrorOut:error_out]) {
        return NO;
    }
    if (!MetafyWaitForReady(self.writer, self.videoInput, @"chunked recorder video frame", error_out)) {
        return NO;
    }

    NSData *frame_data = [NSData dataWithBytesNoCopy:(void *)bytes length:byteCount freeWhenDone:NO];
    CVPixelBufferRef pixel_buffer = NULL;
    if (!MetafyCreateBGRAFramePixelBuffer(
            self.pixelBufferAdaptor,
            self.pixelBufferAttributes,
            self.width,
            self.height,
            &pixel_buffer,
            error_out
        )) {
        return NO;
    }
    MetafyCopyBGRAFrameToPixelBuffer(frame_data, pixel_buffer, self.width, self.height);
    CMTime presentation_time = CMTimeMake(self.currentChunkFrameCount, (int32_t)self.frameRate);
    BOOL appended = [self.pixelBufferAdaptor appendPixelBuffer:pixel_buffer withPresentationTime:presentation_time];
    CVPixelBufferRelease(pixel_buffer);
    if (!appended) {
        MetafySetNSError(error_out, @"Unable to append chunked video frame", self.writer.error);
        return NO;
    }

    self.currentChunkFrameCount += 1;
    self.totalFrameCount += 1;
    if (self.currentChunkFrameCount >= self.maxFramesPerChunk) {
        return [self finishCurrentChunkWithErrorOut:error_out];
    }

    return YES;
}

- (BOOL)finishWithErrorOut:(char **)error_out {
    if (![self finishCurrentChunkWithErrorOut:error_out]) {
        return NO;
    }
    return [self writeManifestWithStatus:@"completed" errorOut:error_out];
}

- (void)cancelOpenChunk {
    if (self.writer != nil) {
        [self.writer cancelWriting];
    }
    self.writer = nil;
    self.videoInput = nil;
    self.pixelBufferAdaptor = nil;
    self.currentChunkFileName = nil;
}

@end

int metafy_macos_segmented_recorder_create(
    const char *manifest_path,
    int64_t width,
    int64_t height,
    int64_t frame_rate,
    int64_t max_frames_per_chunk,
    void **recorder_out,
    char **error_out
) {
    @autoreleasepool {
        if (error_out != NULL) {
            *error_out = NULL;
        }
        if (recorder_out == NULL) {
            MetafySetError(error_out, @"Chunked recorder received no output handle pointer.");
            return 1;
        }
        *recorder_out = NULL;

        NSString *manifest_path_string = MetafyStringFromCString(manifest_path);
        if (manifest_path_string == nil) {
            MetafySetError(error_out, @"Chunked recorder received an invalid UTF-8 manifest path.");
            return 1;
        }
        MetafySegmentedRecorder *recorder = [[MetafySegmentedRecorder alloc] initWithManifestPath:manifest_path_string
                                                                                            width:width
                                                                                           height:height
                                                                                        frameRate:frame_rate
                                                                                maxFramesPerChunk:max_frames_per_chunk
                                                                                        errorOut:error_out];
        if (recorder == nil) {
            return 1;
        }
        *recorder_out = (__bridge_retained void *)recorder;
        return 0;
    }
}

int metafy_macos_segmented_recorder_append_frame(
    void *recorder,
    const uint8_t *bgra_bytes,
    size_t byte_count,
    int64_t elapsed_ms,
    int64_t display_time_ms,
    char **error_out
) {
    @autoreleasepool {
        if (error_out != NULL) {
            *error_out = NULL;
        }
        MetafySegmentedRecorder *segmented_recorder = (__bridge MetafySegmentedRecorder *)recorder;
        if (segmented_recorder == nil) {
            MetafySetError(error_out, @"Chunked recorder append received no recorder handle.");
            return 1;
        }
        return [segmented_recorder appendFrameBytes:bgra_bytes
                                          byteCount:(NSUInteger)byte_count
                                          elapsedMs:elapsed_ms
                                      displayTimeMs:display_time_ms
                                           errorOut:error_out] ? 0 : 1;
    }
}

int metafy_macos_segmented_recorder_finish(void *recorder, char **error_out) {
    @autoreleasepool {
        if (error_out != NULL) {
            *error_out = NULL;
        }
        MetafySegmentedRecorder *segmented_recorder = (__bridge MetafySegmentedRecorder *)recorder;
        if (segmented_recorder == nil) {
            MetafySetError(error_out, @"Chunked recorder finish received no recorder handle.");
            return 1;
        }
        return [segmented_recorder finishWithErrorOut:error_out] ? 0 : 1;
    }
}

void metafy_macos_segmented_recorder_destroy(void *recorder) {
    if (recorder == NULL) {
        return;
    }
    @autoreleasepool {
        MetafySegmentedRecorder *segmented_recorder = (__bridge_transfer MetafySegmentedRecorder *)recorder;
        [segmented_recorder cancelOpenChunk];
    }
}

static NSDictionary *MetafyReadChunkedManifest(NSString *manifest_path, char **error_out) {
    NSData *data = [NSData dataWithContentsOfFile:manifest_path];
    if (data == nil) {
        MetafySetError(error_out, @"Unable to open chunked video manifest at %@.", manifest_path);
        return nil;
    }
    NSError *json_error = nil;
    id value = [NSJSONSerialization JSONObjectWithData:data options:0 error:&json_error];
    if (![value isKindOfClass:[NSDictionary class]]) {
        MetafySetNSError(error_out, @"Unable to parse chunked video manifest", json_error);
        return nil;
    }
    NSDictionary *manifest = (NSDictionary *)value;
    if (![manifest[@"format"] isEqualToString:@"metafy_chunked_h264_segments_v1"]) {
        MetafySetError(error_out, @"Chunked video manifest has an unsupported format.");
        return nil;
    }
    return manifest;
}

static BOOL MetafyBuildChunkComposition(
    NSString *manifest_path,
    NSDictionary *manifest,
    AVMutableComposition **composition_out,
    CMFormatDescriptionRef *source_format_hint_out,
    char **error_out
) {
    NSArray *chunks = manifest[@"chunks"];
    if (![chunks isKindOfClass:[NSArray class]] || chunks.count == 0) {
        MetafySetError(error_out, @"Chunked video manifest does not contain finalized chunks.");
        return NO;
    }

    NSString *manifest_directory = [manifest_path stringByDeletingLastPathComponent];
    AVMutableComposition *composition = [AVMutableComposition composition];
    AVMutableCompositionTrack *composition_track =
        [composition addMutableTrackWithMediaType:AVMediaTypeVideo preferredTrackID:kCMPersistentTrackID_Invalid];
    CMTime cursor = kCMTimeZero;
    CMFormatDescriptionRef source_format_hint = NULL;

    for (NSDictionary *chunk in chunks) {
        if (![chunk isKindOfClass:[NSDictionary class]]) {
            MetafySetError(error_out, @"Chunked video manifest contains a malformed chunk entry.");
            return NO;
        }
        NSString *relative_path = chunk[@"path"];
        if (![relative_path isKindOfClass:[NSString class]] || relative_path.length == 0) {
            MetafySetError(error_out, @"Chunked video manifest contains a chunk without a path.");
            return NO;
        }
        NSString *chunk_path = [manifest_directory stringByAppendingPathComponent:relative_path];
        AVURLAsset *asset = [AVURLAsset URLAssetWithURL:[NSURL fileURLWithPath:chunk_path] options:nil];
        AVAssetTrack *track = [[asset tracksWithMediaType:AVMediaTypeVideo] firstObject];
        if (track == nil) {
            MetafySetError(error_out, @"Chunked video file %@ does not contain a video track.", chunk_path);
            return NO;
        }
        if (source_format_hint == NULL && track.formatDescriptions.count > 0) {
            source_format_hint = (__bridge CMFormatDescriptionRef)track.formatDescriptions[0];
            CFRetain(source_format_hint);
        }
        CMTimeRange time_range = track.timeRange;
        if (CMTIME_IS_INVALID(time_range.duration) || CMTimeCompare(time_range.duration, kCMTimeZero) <= 0) {
            time_range = CMTimeRangeMake(kCMTimeZero, asset.duration);
        }
        NSError *insert_error = nil;
        if (![composition_track insertTimeRange:time_range ofTrack:track atTime:cursor error:&insert_error]) {
            if (source_format_hint != NULL) {
                CFRelease(source_format_hint);
            }
            MetafySetNSError(error_out, @"Unable to add chunked video segment to composition", insert_error);
            return NO;
        }
        cursor = CMTimeAdd(cursor, time_range.duration);
    }

    *composition_out = composition;
    *source_format_hint_out = source_format_hint;
    return YES;
}

static BOOL MetafyEncodePreparedAudioToM4A(
    NSString *audio_path,
    NSString *output_path,
    int64_t sample_rate,
    int64_t channels,
    int64_t max_audio_frames,
    char **error_out
) {
    NSFileHandle *audio_file = [NSFileHandle fileHandleForReadingAtPath:audio_path];
    if (audio_file == nil) {
        MetafySetError(error_out, @"Unable to open prepared f32 audio at %@.", audio_path);
        return NO;
    }

    CMAudioFormatDescriptionRef audio_format_description = MetafyCreateAudioFormatDescription(sample_rate, channels, error_out);
    if (audio_format_description == NULL) {
        [audio_file closeFile];
        return NO;
    }

    NSURL *output_url = [NSURL fileURLWithPath:output_path];
    [[NSFileManager defaultManager] removeItemAtURL:output_url error:nil];
    NSError *writer_error = nil;
    AVAssetWriter *writer = [[AVAssetWriter alloc] initWithURL:output_url fileType:AVFileTypeAppleM4A error:&writer_error];
    if (writer == nil) {
        CFRelease(audio_format_description);
        [audio_file closeFile];
        MetafySetNSError(error_out, @"Unable to create temporary AAC audio writer", writer_error);
        return NO;
    }

    NSDictionary *audio_settings = @{
        AVFormatIDKey: @(kAudioFormatMPEG4AAC),
        AVSampleRateKey: @(sample_rate),
        AVNumberOfChannelsKey: @(channels),
        AVEncoderBitRateKey: @(160000)
    };
    AVAssetWriterInput *audio_input = [AVAssetWriterInput assetWriterInputWithMediaType:AVMediaTypeAudio
                                                                          outputSettings:audio_settings
                                                                        sourceFormatHint:audio_format_description];
    audio_input.expectsMediaDataInRealTime = NO;
    if (![writer canAddInput:audio_input]) {
        CFRelease(audio_format_description);
        [audio_file closeFile];
        MetafySetError(error_out, @"Temporary AAC audio writer cannot add audio input.");
        return NO;
    }
    [writer addInput:audio_input];

    if (![writer startWriting]) {
        CFRelease(audio_format_description);
        [audio_file closeFile];
        MetafySetNSError(error_out, @"Temporary AAC audio writer could not start", writer.error);
        return NO;
    }
    [writer startSessionAtSourceTime:kCMTimeZero];

    int64_t audio_presentation_frame = 0;
    BOOL audio_finished = NO;
    while (!audio_finished) {
        if (!MetafyAppendAudioChunk(
                writer,
                audio_input,
                audio_file,
                audio_format_description,
                sample_rate,
                channels,
                max_audio_frames,
                &audio_presentation_frame,
                &audio_finished,
                error_out
            )) {
            CFRelease(audio_format_description);
            [audio_file closeFile];
            [writer cancelWriting];
            return NO;
        }
    }
    [audio_file closeFile];
    CFRelease(audio_format_description);

    dispatch_semaphore_t finish_semaphore = dispatch_semaphore_create(0);
    [writer finishWritingWithCompletionHandler:^{
        dispatch_semaphore_signal(finish_semaphore);
    }];
    dispatch_semaphore_wait(finish_semaphore, DISPATCH_TIME_FOREVER);
    if (writer.status != AVAssetWriterStatusCompleted) {
        MetafySetNSError(error_out, @"Temporary AAC audio writer did not complete", writer.error);
        return NO;
    }

    return YES;
}

static BOOL MetafyAddAudioAssetToComposition(
    AVMutableComposition *composition,
    NSString *audio_path,
    char **error_out
) {
    AVURLAsset *audio_asset = [AVURLAsset URLAssetWithURL:[NSURL fileURLWithPath:audio_path] options:nil];
    AVAssetTrack *audio_track = [[audio_asset tracksWithMediaType:AVMediaTypeAudio] firstObject];
    if (audio_track == nil) {
        MetafySetError(error_out, @"Temporary AAC audio file %@ does not contain an audio track.", audio_path);
        return NO;
    }

    AVMutableCompositionTrack *composition_audio_track =
        [composition addMutableTrackWithMediaType:AVMediaTypeAudio preferredTrackID:kCMPersistentTrackID_Invalid];
    CMTime duration = audio_track.timeRange.duration;
    if (CMTimeCompare(duration, composition.duration) > 0) {
        duration = composition.duration;
    }
    NSError *insert_error = nil;
    if (![composition_audio_track insertTimeRange:CMTimeRangeMake(kCMTimeZero, duration)
                                         ofTrack:audio_track
                                          atTime:kCMTimeZero
                                           error:&insert_error]) {
        MetafySetNSError(error_out, @"Unable to add temporary AAC audio to chunked composition", insert_error);
        return NO;
    }

    return YES;
}

static BOOL MetafyExportCompositionToMP4(
    AVMutableComposition *composition,
    NSString *output_path,
    char **error_out
) {
    NSURL *output_url = [NSURL fileURLWithPath:output_path];
    [[NSFileManager defaultManager] removeItemAtURL:output_url error:nil];

    AVAssetExportSession *export_session =
        [[AVAssetExportSession alloc] initWithAsset:composition presetName:AVAssetExportPresetPassthrough];
    if (export_session == nil) {
        MetafySetError(error_out, @"Unable to create chunked composition exporter.");
        return NO;
    }
    export_session.outputURL = output_url;
    export_session.outputFileType = AVFileTypeMPEG4;
    export_session.shouldOptimizeForNetworkUse = YES;

    dispatch_semaphore_t export_semaphore = dispatch_semaphore_create(0);
    [export_session exportAsynchronouslyWithCompletionHandler:^{
        dispatch_semaphore_signal(export_semaphore);
    }];
    dispatch_semaphore_wait(export_semaphore, DISPATCH_TIME_FOREVER);
    if (export_session.status != AVAssetExportSessionStatusCompleted) {
        MetafySetNSError(error_out, @"Chunked composition export failed", export_session.error);
        return NO;
    }

    return YES;
}

int metafy_macos_mux_chunked_mp4(
    const char *manifest_path,
    const char *audio_path,
    const char *output_path,
    int64_t audio_sample_rate,
    int64_t audio_channels,
    char **error_out
) {
    @autoreleasepool {
        if (error_out != NULL) {
            *error_out = NULL;
        }
        NSString *manifest_path_string = MetafyStringFromCString(manifest_path);
        NSString *audio_path_string = MetafyStringFromCString(audio_path);
        NSString *output_path_string = MetafyStringFromCString(output_path);
        if (manifest_path_string == nil || output_path_string == nil) {
            MetafySetError(error_out, @"Chunked mux received an invalid UTF-8 manifest or output path.");
            return 1;
        }

        NSDictionary *manifest = MetafyReadChunkedManifest(manifest_path_string, error_out);
        if (manifest == nil) {
            return 1;
        }
        NSNumber *width_number = manifest[@"width"];
        NSNumber *height_number = manifest[@"height"];
        NSNumber *frame_rate_number = manifest[@"frameRate"];
        int64_t width = width_number.longLongValue;
        int64_t height = height_number.longLongValue;
        int64_t frame_rate = frame_rate_number.longLongValue;
        if (width <= 0 || height <= 0 || frame_rate <= 0) {
            MetafySetError(error_out, @"Chunked video manifest has invalid dimensions or frame rate.");
            return 1;
        }
        AVMutableComposition *composition = nil;
        CMFormatDescriptionRef video_format_hint = NULL;
        if (!MetafyBuildChunkComposition(
                manifest_path_string,
                manifest,
                &composition,
                &video_format_hint,
                error_out
            )) {
            return 1;
        }

        if (video_format_hint != NULL) {
            CFRelease(video_format_hint);
            video_format_hint = NULL;
        }

        NSString *temporary_audio_path = nil;
        if (audio_path_string != nil) {
            if (audio_sample_rate <= 0 || audio_channels <= 0) {
                MetafySetError(error_out, @"Chunked mux received invalid audio format metadata.");
                return 1;
            }
            temporary_audio_path = [output_path_string stringByAppendingString:@".audio.m4a"];
            int64_t audio_frame_limit = 0;
            CMTime limited_audio_duration = CMTimeConvertScale(
                composition.duration,
                (int32_t)audio_sample_rate,
                kCMTimeRoundingMethod_RoundHalfAwayFromZero
            );
            if (CMTIME_IS_NUMERIC(limited_audio_duration) && limited_audio_duration.value > 0) {
                audio_frame_limit = limited_audio_duration.value;
            }
            if (!MetafyEncodePreparedAudioToM4A(
                    audio_path_string,
                    temporary_audio_path,
                    audio_sample_rate,
                    audio_channels,
                    audio_frame_limit,
                    error_out
                )) {
                [[NSFileManager defaultManager] removeItemAtPath:temporary_audio_path error:nil];
                return 1;
            }
            if (!MetafyAddAudioAssetToComposition(composition, temporary_audio_path, error_out)) {
                [[NSFileManager defaultManager] removeItemAtPath:temporary_audio_path error:nil];
                return 1;
            }
        }

        BOOL export_ok = MetafyExportCompositionToMP4(composition, output_path_string, error_out);
        if (temporary_audio_path != nil) {
            [[NSFileManager defaultManager] removeItemAtPath:temporary_audio_path error:nil];
        }
        return export_ok ? 0 : 1;
    }
}

int metafy_macos_encode_mp4(
    const char *video_path,
    const char *audio_path,
    const char *output_path,
    int64_t width,
    int64_t height,
    int64_t frame_rate,
    int64_t frame_count,
    int64_t audio_sample_rate,
    int64_t audio_channels,
    char **error_out
) {
    @autoreleasepool {
        if (error_out != NULL) {
            *error_out = NULL;
        }
        if (video_path == NULL || output_path == NULL) {
            MetafySetError(error_out, @"Native macOS encoder received missing video or output path.");
            return 1;
        }
        if (width <= 0 || height <= 0 || frame_rate <= 0 || frame_count <= 0) {
            MetafySetError(error_out, @"Native macOS encoder received invalid video dimensions or timeline.");
            return 1;
        }
        if (width > INT64_MAX / height || width * height > INT64_MAX / 4) {
            MetafySetError(error_out, @"Native macOS encoder video dimensions are too large.");
            return 1;
        }
        NSUInteger frame_byte_count = (NSUInteger)(width * height * 4);

        NSString *video_path_string = MetafyStringFromCString(video_path);
        NSString *audio_path_string = MetafyStringFromCString(audio_path);
        NSString *output_path_string = MetafyStringFromCString(output_path);
        if (video_path_string == nil || output_path_string == nil) {
            MetafySetError(error_out, @"Native macOS encoder received an invalid UTF-8 path.");
            return 1;
        }

        NSFileHandle *video_file = [NSFileHandle fileHandleForReadingAtPath:video_path_string];
        if (video_file == nil) {
            MetafySetError(error_out, @"Unable to open prepared BGRA video at %@.", video_path_string);
            return 1;
        }

        NSFileHandle *audio_file = nil;
        CMAudioFormatDescriptionRef audio_format_description = NULL;
        if (audio_path_string != nil) {
            if (audio_sample_rate <= 0 || audio_channels <= 0) {
                MetafySetError(error_out, @"Native macOS encoder received invalid audio format metadata.");
                return 1;
            }
            audio_file = [NSFileHandle fileHandleForReadingAtPath:audio_path_string];
            if (audio_file == nil) {
                MetafySetError(error_out, @"Unable to open prepared f32 audio at %@.", audio_path_string);
                return 1;
            }
            audio_format_description = MetafyCreateAudioFormatDescription(audio_sample_rate, audio_channels, error_out);
            if (audio_format_description == NULL) {
                return 1;
            }
        }

        NSURL *output_url = [NSURL fileURLWithPath:output_path_string];
        [[NSFileManager defaultManager] removeItemAtURL:output_url error:nil];

        NSError *writer_error = nil;
        AVAssetWriter *writer = [[AVAssetWriter alloc] initWithURL:output_url fileType:AVFileTypeMPEG4 error:&writer_error];
        if (writer == nil) {
            if (audio_format_description != NULL) {
                CFRelease(audio_format_description);
            }
            MetafySetNSError(error_out, @"Unable to create AVAssetWriter", writer_error);
            return 1;
        }
        writer.shouldOptimizeForNetworkUse = YES;

        NSDictionary *video_settings = @{
            AVVideoCodecKey: AVVideoCodecTypeH264,
            AVVideoWidthKey: @(width),
            AVVideoHeightKey: @(height),
            AVVideoCompressionPropertiesKey: @{
                AVVideoAverageBitRateKey: @((NSInteger)MAX(1, width * height * frame_rate / 2)),
                AVVideoProfileLevelKey: AVVideoProfileLevelH264HighAutoLevel
            }
        };
        AVAssetWriterInput *video_input = [AVAssetWriterInput assetWriterInputWithMediaType:AVMediaTypeVideo outputSettings:video_settings];
        video_input.expectsMediaDataInRealTime = NO;
        if (![writer canAddInput:video_input]) {
            if (audio_format_description != NULL) {
                CFRelease(audio_format_description);
            }
            MetafySetError(error_out, @"AVAssetWriter cannot add the H.264 video input.");
            return 1;
        }
        [writer addInput:video_input];

        NSDictionary *pixel_buffer_attributes = @{
            (NSString *)kCVPixelBufferPixelFormatTypeKey: @(kCVPixelFormatType_32BGRA),
            (NSString *)kCVPixelBufferWidthKey: @(width),
            (NSString *)kCVPixelBufferHeightKey: @(height),
            (NSString *)kCVPixelBufferIOSurfacePropertiesKey: @{}
        };
        AVAssetWriterInputPixelBufferAdaptor *pixel_buffer_adaptor =
            [AVAssetWriterInputPixelBufferAdaptor assetWriterInputPixelBufferAdaptorWithAssetWriterInput:video_input
                                                                             sourcePixelBufferAttributes:pixel_buffer_attributes];

        AVAssetWriterInput *audio_input = nil;
        if (audio_file != nil) {
            NSDictionary *audio_settings = @{
                AVFormatIDKey: @(kAudioFormatMPEG4AAC),
                AVSampleRateKey: @(audio_sample_rate),
                AVNumberOfChannelsKey: @(audio_channels),
                AVEncoderBitRateKey: @(160000)
            };
            audio_input = [AVAssetWriterInput assetWriterInputWithMediaType:AVMediaTypeAudio
                                                              outputSettings:audio_settings
                                                            sourceFormatHint:audio_format_description];
            audio_input.expectsMediaDataInRealTime = NO;
            if (![writer canAddInput:audio_input]) {
                CFRelease(audio_format_description);
                MetafySetError(error_out, @"AVAssetWriter cannot add the AAC audio input.");
                return 1;
            }
            [writer addInput:audio_input];
        }

        if (![writer startWriting]) {
            if (audio_format_description != NULL) {
                CFRelease(audio_format_description);
            }
            MetafySetNSError(error_out, @"AVAssetWriter could not start writing", writer.error);
            return 1;
        }
        [writer startSessionAtSourceTime:kCMTimeZero];

        BOOL media_ok = MetafyAppendMedia(
            writer,
            video_input,
            pixel_buffer_adaptor,
            pixel_buffer_attributes,
            video_file,
            width,
            height,
            frame_rate,
            frame_count,
            frame_byte_count,
            audio_input,
            audio_file,
            audio_format_description,
            audio_sample_rate,
            audio_channels,
            error_out
        );
        [video_file closeFile];
        if (audio_file != nil) {
            [audio_file closeFile];
        }
        if (audio_format_description != NULL) {
            CFRelease(audio_format_description);
            audio_format_description = NULL;
        }
        if (!media_ok) {
            [writer cancelWriting];
            return 1;
        }

        dispatch_semaphore_t finish_semaphore = dispatch_semaphore_create(0);
        [writer finishWritingWithCompletionHandler:^{
            dispatch_semaphore_signal(finish_semaphore);
        }];
        dispatch_semaphore_wait(finish_semaphore, DISPATCH_TIME_FOREVER);

        if (writer.status != AVAssetWriterStatusCompleted) {
            MetafySetNSError(error_out, @"AVAssetWriter did not complete", writer.error);
            return 1;
        }

        return 0;
    }
}

void metafy_macos_free_string(char *value) {
    free(value);
}
