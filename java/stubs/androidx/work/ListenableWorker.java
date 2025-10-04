package androidx.work;

import android.content.Context;

public abstract class ListenableWorker {
    private Context context;
    private WorkerParameters params;

    public ListenableWorker(Context context, WorkerParameters params) {
        this.context = context;
        this.params = params;
    }

    public Context getApplicationContext() {
        return context;
    }

    public Data getInputData() {
        return params.getInputData();
    }

    public void setProgressAsync(Data data) {
        // Stub implementation
    }

    public void setForegroundAsync(ForegroundInfo foregroundInfo) {
        // Stub implementation
    }

    public static abstract class Result {
        public static Result success() {
            return new Success();
        }

        public static Result success(Data data) {
            return new Success(data);
        }

        public static Result failure() {
            return new Failure();
        }

        public static Result failure(Data data) {
            return new Failure(data);
        }

        public static Result retry() {
            return new Retry();
        }

        private static class Success extends Result {
            Success() {}
            Success(Data data) {}
        }

        private static class Failure extends Result {
            Failure() {}
            Failure(Data data) {}
        }

        private static class Retry extends Result {}
    }
}
