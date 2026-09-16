# Changelog

---
## [0.5.3](https://github.com/wizrds/guestpy/compare/0.5.2..0.5.3) - 2026-09-16

### Bug Fixes

- Support instantiating host class instances in host - ([4b2b33d](https://github.com/wizrds/guestpy/commit/4b2b33d5abdd34da70b6f664df16f35252702d6c)) - Timothy Pogue
---
## [0.5.2](https://github.com/wizrds/guestpy/compare/0.5.1..0.5.2) - 2026-09-16

### Bug Fixes

- Add construct_with for instantiating classes with keyword arguments - ([a7b1335](https://github.com/wizrds/guestpy/commit/a7b1335375e624eb68a12ea3899ca0cca3087903)) - Timothy Pogue
---
## [0.5.1](https://github.com/wizrds/guestpy/compare/0.5.0..0.5.1) - 2026-09-15

### Bug Fixes

- Avoid Debug bound on Raise args for handle access - ([8534ae8](https://github.com/wizrds/guestpy/commit/8534ae857ddc4f684efa12e8e6d77b4c7098fd61)) - Timothy Pogue
---
## [0.5.0](https://github.com/wizrds/guestpy/compare/0.4.0..0.5.0) - 2026-09-15

### Features

-  [**breaking**]Improve receiver handling in host classes, expose module state to host classes, and support unions in data structures  - ([49d2c2f](https://github.com/wizrds/guestpy/commit/49d2c2f28a8419858317164129a6fe3817f143c8)) - Timothy Pogue
---
## [0.4.0](https://github.com/wizrds/guestpy/compare/0.3.1..0.4.0) - 2026-09-14

### Bug Fixes

- Make Guest::enter public API - ([2126a0c](https://github.com/wizrds/guestpy/commit/2126a0c99cfd7d52fbff3dd7191ffa0a1344a56d)) - Timothy Pogue
- Support native inheritance and class semantics for host classes - ([d0dd720](https://github.com/wizrds/guestpy/commit/d0dd7204276783bdd40671ef51241eb7c115ccc1)) - Timothy Pogue
- Fix stop iteration handling in async iter and async generator - ([2ad684e](https://github.com/wizrds/guestpy/commit/2ad684e8da54f69d4eb6d643a2c4ea1ba6eeee50)) - Timothy Pogue
- Improve dunder handling in class definitions and make class builder fallible - ([98aebdc](https://github.com/wizrds/guestpy/commit/98aebdccb0b35861f1ac7864948280c2e71c8f37)) - Timothy Pogue
- Add host exception registration and macro - ([7eebeba](https://github.com/wizrds/guestpy/commit/7eebebaa8771aef1f5c12701be494d2c2dac336b)) - Timothy Pogue
- Rework exception handling and allow raising and catching from host side - ([0c22af0](https://github.com/wizrds/guestpy/commit/0c22af09f746c1aaa84407339682b89218b73732)) - Timothy Pogue

### Features

- Improve handling and exposing exceptions and improve dunder handling in host classes  - ([ae1fe39](https://github.com/wizrds/guestpy/commit/ae1fe39873d5603bd0fa68fcc2a95f07f66271ef)) - Timothy Pogue

### Miscellaneous

- Fix formatting - ([cc36caa](https://github.com/wizrds/guestpy/commit/cc36caa0ba435d8a5b972de65260188f7f1db7a4)) - Timothy Pogue
- Update README and lib docstring - ([d91e0a4](https://github.com/wizrds/guestpy/commit/d91e0a4b5be83e3013030b4a6aeacad05399e3f3)) - Timothy Pogue
- Fix formatting - ([7b25126](https://github.com/wizrds/guestpy/commit/7b25126affa56360bef9c4806938cd5e807a5473)) - Timothy Pogue
- Add backend exceptions test fixtures - ([3aabae1](https://github.com/wizrds/guestpy/commit/3aabae13b6eddafadff50d925376bc7d90852d28)) - Timothy Pogue
---
## [0.3.1](https://github.com/wizrds/guestpy/compare/0.3.0..0.3.1) - 2026-09-11

### Bug Fixes

- Delegate to python import in host import method when target is not bound directly - ([ec3860b](https://github.com/wizrds/guestpy/commit/ec3860bc8f43bff886a17268cec2262fadfb589d)) - Timothy Pogue
---
## [0.3.0](https://github.com/wizrds/guestpy/compare/0.2.1..0.3.0) - 2026-09-02

### Bug Fixes

- **(macros)** Better handling of user defined bounds in host classes for host_class macro - ([1777352](https://github.com/wizrds/guestpy/commit/1777352b8a7f320661bca2ed63c590b6bf680217)) - Timothy Pogue
- Rework this receiver in host_class macro and rename subscriptable to generic - ([5dfdd46](https://github.com/wizrds/guestpy/commit/5dfdd4675190e338fb7e9cbed34d2839385e3772)) - Timothy Pogue
- Rename subscriptable to generic - ([55ba5c6](https://github.com/wizrds/guestpy/commit/55ba5c6d6df63608539c396f1670fb43d371b032)) - Timothy Pogue
- Rework backend attribute on host class macro - ([99649d1](https://github.com/wizrds/guestpy/commit/99649d1b2189068be3985a9dcc98038ae91479b5)) - Timothy Pogue
- Export macros in prelude - ([8471147](https://github.com/wizrds/guestpy/commit/8471147ba6ab6d4858d399318a265aaa0117483f)) - Timothy Pogue
- Support host class instantiation directly on host - ([c106477](https://github.com/wizrds/guestpy/commit/c106477d928402a772349aaf653674c66a636abe)) - Timothy Pogue
- Support generic host class implementations in macros - ([ae8babb](https://github.com/wizrds/guestpy/commit/ae8babbd42163a406d0bc04b7ed7e9ad6f1f5bdc)) - Timothy Pogue
- Move construct to host class definition - ([967d4b4](https://github.com/wizrds/guestpy/commit/967d4b4f2f14f8793c66eb8f6d846d959da880b7)) - Timothy Pogue
- Fix dir in rustpython impl and improve classes test fixtures - ([9ee48b3](https://github.com/wizrds/guestpy/commit/9ee48b3ef2c489f168afe81c55dbbdbcce71d63d)) - Timothy Pogue
- Add support for subscriptable host classes and guest handle access in host class methods - ([7ea514e](https://github.com/wizrds/guestpy/commit/7ea514ecf0ed7ed2961cf48cdf76411a66654a03)) - Timothy Pogue
- Rework handle APIs to reduce repetition and expose machinery - ([a5ca9e7](https://github.com/wizrds/guestpy/commit/a5ca9e7a24d766d3e218a16b8aa1a07b0ffc2009)) - Timothy Pogue

### Features

- Rework handle method access and add generic host class support  - ([7155dac](https://github.com/wizrds/guestpy/commit/7155dacf631b033085a8265ddeb9d362683eef99)) - Timothy Pogue

### Miscellaneous

- Fix formatting - ([33e10da](https://github.com/wizrds/guestpy/commit/33e10dae3e167149c3ed857508a63d550f2ffacd)) - Timothy Pogue
---
## [0.2.1](https://github.com/wizrds/guestpy/compare/0.2.0..0.2.1) - 2026-08-30

### Bug Fixes

- Support crate_path attribute in derive macros - ([dd0951a](https://github.com/wizrds/guestpy/commit/dd0951aedb76e5ca02df8644faadf82191e97e32)) - Timothy Pogue
---
## [0.2.0](https://github.com/wizrds/guestpy/compare/0.1.1..0.2.0) - 2026-08-29

### Bug Fixes

- Bump rustpython for malachite 0.11 - ([294414f](https://github.com/wizrds/guestpy/commit/294414fca701ab8c7ddbe803e34dd9348683b5ab)) - Timothy Pogue
- Support crate_path param in bundle macro - ([8b5545e](https://github.com/wizrds/guestpy/commit/8b5545e49c6236ab1095fc28416741c7b55d69d5)) - Timothy Pogue
- Add native extension support in bundles - ([3cd4aa8](https://github.com/wizrds/guestpy/commit/3cd4aa871c8d704033f5c3329b6c788f4971d922)) - Timothy Pogue
- Expose import method on Scope - ([df0b9af](https://github.com/wizrds/guestpy/commit/df0b9af969f5988c76594b1718d1a3a7796532a7)) - Timothy Pogue
- Fix callables unit test fixtures - ([794e07a](https://github.com/wizrds/guestpy/commit/794e07a477e231427fa49f6dbe078c8bac57879a)) - Timothy Pogue
- Expose import method on guest - ([eb6d285](https://github.com/wizrds/guestpy/commit/eb6d285409e945ab5ae470d204a66840b1e532e8)) - Timothy Pogue

### Features

- Add direct support for C based Python packages in PyO3 backend  - ([2dbabf7](https://github.com/wizrds/guestpy/commit/2dbabf7db9d244a5a6e3d389be697393a878438f)) - Timothy Pogue

### Miscellaneous

- Fix linting problems - ([688d1cd](https://github.com/wizrds/guestpy/commit/688d1cdc99d77c54f9d2c93a0ef4af32bd116e8a)) - Timothy Pogue
- Fix formatting - ([5b4b716](https://github.com/wizrds/guestpy/commit/5b4b716d153fbde92101f4a1624685d1465f04c9)) - Timothy Pogue
- Update README - ([a74f2b8](https://github.com/wizrds/guestpy/commit/a74f2b8446230aadba0bc79b76584c557b1a5688)) - Timothy Pogue
---
## [0.1.1](https://github.com/wizrds/guestpy/compare/0.1.0..0.1.1) - 2026-08-28

### Bug Fixes

- Replace bytes newtype wrapper with bytes crate integration - ([8df9b7a](https://github.com/wizrds/guestpy/commit/8df9b7acf8ec39c5de196e586e2c2bbb7b1533bd)) - Timothy Pogue
---
## [0.1.0] - 2026-08-28

### Features

- Initial project setup :tada: - ([5045862](https://github.com/wizrds/guestpy/commit/50458620fd6bba6ca90cd6ddc77f0bd4e28bc196)) - Timothy Pogue

