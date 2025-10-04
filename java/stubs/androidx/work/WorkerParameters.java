package androidx.work;

import java.util.UUID;

public class WorkerParameters {
    private UUID id;
    private Data inputData;

    public WorkerParameters(UUID id, Data inputData) {
        this.id = id;
        this.inputData = inputData;
    }

    public UUID getId() {
        return id;
    }

    public Data getInputData() {
        return inputData;
    }
}
