// JniBridge.java
package com.example.android;

import android.app.Activity;
import android.content.Context;
import androidx.core.app.ActivityCompat;
import androidx.core.content.ContextCompat;

public class JniBridge {
    public JniBridge() {
        System.out.println("JniBridge constructor called");
    }

    public int checkSelfPermission(Context context, String permission) {
        return ContextCompat.checkSelfPermission(context, permission);
    }

    public int requestPermission(Activity activity, String permission) {
        activity.requestPermissions(new String[] { permission }, 0);
        return 0;
    }
}