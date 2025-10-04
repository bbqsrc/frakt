package androidx.work;

public class WorkInfo {
    public static enum State {
        ENQUEUED, RUNNING, SUCCEEDED, FAILED, BLOCKED, CANCELLED
    }

    public State getState() {
        return State.SUCCEEDED;
    }

    public Data getOutputData() {
        return new Data.Builder().build();
    }
}
