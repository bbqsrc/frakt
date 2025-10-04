package androidx.work;

public class Configuration {
    public static class Builder {
        private WorkerFactory workerFactory;

        public Builder() {}

        public Builder setWorkerFactory(WorkerFactory workerFactory) {
            this.workerFactory = workerFactory;
            return this;
        }

        public Configuration build() {
            return new Configuration();
        }
    }
}
