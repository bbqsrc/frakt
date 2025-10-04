package se.brendan.frakt;

public class DownloadProgressCallback {
    private final long handlerId;

    public DownloadProgressCallback(long handlerId) {
        this.handlerId = handlerId;
    }

    public void onProgress(long bytesDownloaded, long totalBytes) {
        // Call native method to invoke Rust callback
        nativeOnProgress(handlerId, bytesDownloaded, totalBytes);
    }

    private static native void nativeOnProgress(long handlerId, long bytesDownloaded, long totalBytes);
}
