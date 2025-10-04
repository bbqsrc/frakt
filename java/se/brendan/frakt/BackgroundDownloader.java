package se.brendan.frakt;

import android.content.Context;

/**
 * Simple synchronous downloader for background downloads.
 * In a real Android app, this would be wrapped in a WorkManager Worker,
 * but for testing we just call it directly.
 */
public class BackgroundDownloader {

    public static int performDownload(
            Context context,
            String url,
            String filePath,
            String headersJson,
            DownloadProgressCallback progressCallback) {

        System.out.println("🚀 BackgroundDownloader.performDownload() called");
        System.out.println("   URL: " + url);
        System.out.println("   File: " + filePath);

        // Call native download method (context is not needed for our implementation)
        return nativeDownload(url, filePath, headersJson, progressCallback);
    }

    private static native int nativeDownload(
            String url,
            String filePath,
            String headersJson,
            DownloadProgressCallback callback);
}
