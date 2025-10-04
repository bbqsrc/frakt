package androidx.core.app;

import android.app.Notification;
import android.content.Context;

public class NotificationCompat {
    public static class Builder {
        public Builder(Context context, String channelId) {}
        public Builder setContentTitle(CharSequence title) { return this; }
        public Builder setContentText(CharSequence text) { return this; }
        public Builder setSmallIcon(int icon) { return this; }
        public Builder setOngoing(boolean ongoing) { return this; }
        public Notification build() { return new Notification(); }
    }
}
