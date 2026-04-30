# Changelog

## [0.1.2] - 2026-04-30

### Security Fixes

- **Fix broken logic for enabling HTTPS only in release mode.**
- **Fix broken authentication check logic that allows invalid tokens to clear the internal cache.**

## [0.1.1] - 2026-04-30

### Features

- Improve compliance with blob cid specification by only accepting v1 hashes with accepted codecs

### Performance

- Use jemallocator instead of the system allocator on all plat forms.

### Security Fixes

- Make authentication checks constant time.

### Other

- Refactored codebase for future maintainability


## [0.1.0] - 2026-03-26

- Initial Release
