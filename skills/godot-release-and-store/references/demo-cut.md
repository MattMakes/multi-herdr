# Demo cut by export plugin

Read this when you cut a demo or another edition that must leave full-game content out of the pack. Step 9 of the main method in `SKILL.md` points here.

## Demo cut by export plugin

A demo cut must leave full-game content out of the pack, not only lock it in the menu. A player
can open an unprotected pack with public tools.

1. Map how the game loads content: data rows, scenes, art, audio, and generated lists.
2. Write the demo set as data: the rows and the file globs to keep.
3. Write an `EditorExportPlugin`.
   - In `_export_file`, call `skip()` for each full-game file glob.
   - For a data file, filter the full-game rows out, then add the filtered copy with
     `add_file`.
   - Act only when the export features hold the demo feature.
   - Register the plugin from an editor plugin, and enable that addon in the project. A headless
     export runs only enabled plugins.
4. Change each `preload` of a full-game file in a kept script to a guarded load.
   `preload` fails at parse time when the file is gone. A guarded load checks
   `ResourceLoader.exists` first.
5. Export the full edition from the changed tree. Compare its file list with the old full
   export. The plugin must not change the full build.
6. Export the demo. Run the pack audit.
7. Read the audit for shared names. A filter by name prefix can cut a key that both editions
   use. Exempt shared names explicitly.
8. Add compiled script files (`.gdc`) and import caches under full-game folders to the skip
   globs.
9. Search the asset file names in the demo pack. A file name such as a preview image can
   reveal cut content. Rename or remove it.
10. Zip the demo. Unzip it in a temp folder and run it once (Method step 12).
