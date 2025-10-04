package se.brendan.frakt;

import android.content.Context;
import androidx.annotation.NonNull;
import androidx.work.Data;
import androidx.work.Worker;
import androidx.work.WorkerParameters;

public class DownloadWorker extends Worker {

    // Note: Don't load library here - it's already loaded by the main app
    // The DEX classloader doesn't have access to native library directories

    public DownloadWorker(@NonNull Context context, @NonNull WorkerParameters params) {
        super(context, params);
        System.out.println("✅ DownloadWorker constructor completed");
        System.out.flush();
        System.err.println("✅ DownloadWorker constructor completed (stderr)");
        System.err.flush();
    }

    @NonNull
    @Override
    public Result doWork() {
        System.err.println("🚀🚀🚀 ENTERED doWork() - STDERR - VERY FIRST LINE 🚀🚀🚀");
        System.err.flush();
        System.out.println("🚀 ENTERED doWork() - STDOUT - VERY FIRST LINE");
        System.out.flush();
        try {
            System.out.println("🔧 DownloadWorker.doWork() in try block");

            // Get input data
            String url = getInputData().getString("url");
            String filePath = getInputData().getString("file_path");
            String headersJson = getInputData().getString("headers");

            System.out.println("🔧 Got input data - calling doWork() body");
            System.out.println("   URL: " + url);
            System.out.println("   File: " + filePath);

            if (url == null || filePath == null) {
                System.err.println("❌ URL or file path is null!");
                return Result.failure();
            }

            // Get progress handler ID if present
            long progressHandlerId = getInputData().getLong("progress_handler_id", -1);
            DownloadProgressCallback progressCallback = null;

            if (progressHandlerId != -1) {
                System.out.println("📊 Progress handler ID: " + progressHandlerId);
                progressCallback = new DownloadProgressCallback(progressHandlerId);
            }

            // Call native download function
            System.out.println("📞 Calling nativeDownload...");
            int result = nativeDownload(url, filePath, headersJson != null ? headersJson : "{}", progressCallback);
            System.out.println("📞 nativeDownload returned: " + result);

            if (result == 0) {
                System.out.println("✅ Download succeeded");
                return Result.success();
            } else {
                System.err.println("❌ Download failed with code: " + result);
                Data failureData = new Data.Builder()
                        .putInt("error_code", result)
                        .build();
                return Result.failure(failureData);
            }
        } catch (Exception e) {
            System.err.println("❌ Exception in DownloadWorker.doWork(): " + e.getMessage());
            e.printStackTrace();
            Data failureData = new Data.Builder()
                    .putString("error", e.getMessage())
                    .build();
            return Result.failure(failureData);
        }
    }

    private native int nativeDownload(String url, String filePath, String headersJson, DownloadProgressCallback callback);
}
