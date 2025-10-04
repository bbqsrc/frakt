package se.brendan.frakt;

import android.content.Context;
import androidx.annotation.NonNull;
import androidx.work.ListenableWorker;
import androidx.work.WorkerFactory;
import androidx.work.WorkerParameters;

public class DexWorkerFactory extends WorkerFactory {
    private ClassLoader dexClassLoader;

    public DexWorkerFactory(ClassLoader dexClassLoader) {
        this.dexClassLoader = dexClassLoader;
    }

    @Override
    public ListenableWorker createWorker(
            @NonNull Context appContext,
            @NonNull String workerClassName,
            @NonNull WorkerParameters workerParameters) {

        System.out.println("🏭 DexWorkerFactory.createWorker() called for: " + workerClassName);

        try {
            // Load class from DEX classloader
            Class<?> workerClass = Class.forName(workerClassName, true, dexClassLoader);
            System.out.println("✅ Loaded class from DEX: " + workerClassName);

            // Create instance
            ListenableWorker worker = (ListenableWorker) workerClass
                    .getConstructor(Context.class, WorkerParameters.class)
                    .newInstance(appContext, workerParameters);

            System.out.println("✅ Created worker instance: " + worker.getClass().getName());
            return worker;

        } catch (Exception e) {
            System.err.println("❌ Failed to create worker: " + e.getMessage());
            e.printStackTrace();
            return null;
        }
    }
}
