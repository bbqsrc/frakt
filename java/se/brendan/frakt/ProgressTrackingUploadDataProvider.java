package se.brendan.frakt;

import org.chromium.net.UploadDataProvider;
import org.chromium.net.UploadDataSink;
import java.nio.ByteBuffer;

public class ProgressTrackingUploadDataProvider extends UploadDataProvider {
    private final byte[] data;
    private final long progressHandlerId;
    private long bytesUploaded = 0;

    public ProgressTrackingUploadDataProvider(byte[] data, long progressHandlerId) {
        this.data = data;
        this.progressHandlerId = progressHandlerId;
    }

    @Override
    public long getLength() {
        return data.length;
    }

    @Override
    public void read(UploadDataSink uploadDataSink, ByteBuffer byteBuffer) {
        try {
            int remaining = (int) (data.length - bytesUploaded);
            int toRead = Math.min(remaining, byteBuffer.remaining());

            if (toRead > 0) {
                byteBuffer.put(data, (int) bytesUploaded, toRead);
                bytesUploaded += toRead;

                if (progressHandlerId != -1) {
                    nativeOnUploadProgress(progressHandlerId, bytesUploaded, data.length);
                }
            }

            // For non-chunked uploads (when getLength() returns a value),
            // always pass false to onReadSucceeded. Cronet tracks completion automatically.
            uploadDataSink.onReadSucceeded(false);
        } catch (Exception e) {
            uploadDataSink.onReadError(e);
        }
    }

    @Override
    public void rewind(UploadDataSink uploadDataSink) {
        try {
            bytesUploaded = 0;
            uploadDataSink.onRewindSucceeded();
        } catch (Exception e) {
            uploadDataSink.onRewindError(e);
        }
    }

    private static native void nativeOnUploadProgress(long handlerId, long bytesUploaded, long totalBytes);
}
