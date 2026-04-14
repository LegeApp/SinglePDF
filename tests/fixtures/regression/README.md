Run the local regression harness with:

```powershell
cargo test --test regression_fixtures -- --ignored --nocapture
```

The fixture list and expected ranges live in `manifest.json`.
You can override the manifest location via `SINGLEPDF_REGRESSION_MANIFEST`.
