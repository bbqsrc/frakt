package android.content;

public abstract class Context {
    public static final String NOTIFICATION_SERVICE = "notification";
    public abstract Object getSystemService(String name);
}
