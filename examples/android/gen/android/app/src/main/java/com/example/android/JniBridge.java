// JniBridge.java
package com.example.android;

import android.content.Context;
import androidx.core.content.ContextCompat;

public class JniBridge {
    public JniBridge() {
        System.out.println("JniBridge constructor called");
    }

    public int checkSelfPermission(Context context, String permission) {
        return ContextCompat.checkSelfPermission(context, permission);
    }
}