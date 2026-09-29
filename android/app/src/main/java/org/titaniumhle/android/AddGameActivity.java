/*
 * SPDX-License-Identifier: MPL-2.0
 * Copyright © 2026 Gitsnup and touchHLE contributors
 */

package org.titaniumhle.android;

import android.app.Activity;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.os.Bundle;
import android.provider.OpenableColumns;
import android.widget.Toast;
import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;

/**
 * An in-app game importer, shown when the user taps the "Add game" button in
 * touchHLE's app picker. This replaces the old approach of sending users to
 * their file manager app, which modern Android versions no longer permit for
 * an app's private storage.
 *
 * It is started via the {@code titaniumhle://add-game} deep link (see
 * AndroidManifest.xml and paths.rs), immediately opens the system document
 * picker, and copies the picked file into touchHLE's apps directory.
 */
public class AddGameActivity extends Activity {
    private static final int REQUEST_OPEN_GAME = 1;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        Uri intent_url = getIntent().getData();
        if (intent_url == null || !"add-game".equals(intent_url.getHost())) {
            finish();
            return;
        }

        Intent picker = new Intent(Intent.ACTION_OPEN_DOCUMENT);
        picker.addCategory(Intent.CATEGORY_OPENABLE);
        picker.setType("*/*");
        picker.putExtra(Intent.EXTRA_MIME_TYPES, new String[] {
            "application/octet-stream",
            "application/zip",
            "application/x-zip-compressed",
        });
        picker.putExtra(Intent.EXTRA_TITLE, "YourGame.ipa");

        try {
            startActivityForResult(picker, REQUEST_OPEN_GAME);
        } catch (Exception e) {
            toast("Couldn't open a file picker: " + e.getMessage());
            finish();
        }
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);

        if (requestCode != REQUEST_OPEN_GAME || resultCode != RESULT_OK || data == null
                || data.getData() == null) {
            finish();
            return;
        }

        Uri game_url = data.getData();
        try {
            String file_name = get_display_name(game_url);
            if (file_name == null || file_name.isEmpty()) {
                file_name = "game.ipa";
            }
            // Don't let a weird display name escape the apps directory.
            file_name = file_name.replaceAll("[/\\\\]", "_");

            File apps_dir = new File(getExternalFilesDir(null), "touchHLE_apps");
            apps_dir.mkdirs();
            File dest = new File(apps_dir, file_name);
            copy_stream(getContentResolver().openInputStream(game_url), dest);
            // The app picker only scans for games at startup, so a restart is
            // needed for the new game to show up.
            toast("Imported " + file_name + ". Restart touchHLE to see it.");
        } catch (IOException | SecurityException e) {
            toast("Couldn't import the file: " + e.getMessage());
        }

        finish();
    }

    private String get_display_name(Uri url) {
        String[] projection = { OpenableColumns.DISPLAY_NAME };
        try (Cursor cursor = getContentResolver().query(url, projection, null, null, null)) {
            if (cursor != null && cursor.moveToFirst()) {
                int column = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME);
                if (column >= 0) {
                    return cursor.getString(column);
                }
            }
        }
        return null;
    }

    private void copy_stream(InputStream input, File dest) throws IOException {
        try (OutputStream output = new FileOutputStream(dest)) {
            byte[] buffer = new byte[65536];
            int bytes_read;
            while ((bytes_read = input.read(buffer)) >= 0) {
                output.write(buffer, 0, bytes_read);
            }
        } finally {
            input.close();
        }
    }

    private void toast(String message) {
        Toast.makeText(this, message, Toast.LENGTH_LONG).show();
    }
}
