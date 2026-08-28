# GuestPy

GuestPy lets a Rust application load and run Python code inside an interpreter, call the
functions and read the values that code defines, and expose Rust functions, classes, and modules for
that Python code to call back into. GuestPy has no interpreter of its own and instead runs on a pluggable
backend, so the same host code works unchanged against either of two interchangeable backends: real
CPython or an all-Rust interpreter, requiring no separate Python installation.

The library provides:

- isolated guests sharing one runtime;
- direct Rust-to-Python and Python-to-Rust calls;
- typed guest-module facades;
- `Bundle` source packages;
- serde-backed conversion for ordinary Rust data;
- macros for defining Rust host classes and host modules;
- awaitable guest coroutines;
- timeouts and cancellation controls; and
- runtime-global and guest-local capability binding.

GuestPy does not manage Python package installation or provide filesystem source loading.
Applications remain responsible for choosing guest source and deciding which capabilities each guest
receives.

## Installation

Add GuestPy to your project and enable one backend feature: `pyo3` for real CPython, or
`rustpython` for RustPython. Either feature alone is enough; GuestPy re-exports the chosen
backend's concrete type through its own crate, so ordinary embedding needs no second,
backend-specific dependency:

```toml
[dependencies]
guestpy = { git = "https://github.com/wizrds/guestpy", features = ["pyo3"] }
```

To use RustPython instead:

```toml
[dependencies]
guestpy = { git = "https://github.com/wizrds/guestpy", features = ["rustpython"] }
```

GuestPy has no default features. Enable only the optional behavior the application needs:

| Feature | Behavior |
| --- | --- |
| `pyo3` | Compiles in the `guestpy-pyo3` backend crate (real CPython, via the `pyo3` crate) and re-exports it as `guestpy::pyo3`. |
| `rustpython` | Compiles in the `guestpy-rustpython` backend crate and re-exports it as `guestpy::rustpython`. |
| `embedded` | Enables embedded guest-source bundles. |
| `serde` | Enables serde-backed Rust data conversion. |
| `tokio` | Enables Tokio cancellation support and filesystem guest-source bundles. |
| `bytes` | Enables `Bytes` conversion for guest data. |

## Quick start

Build a runtime, build a guest, load Python source, and call its exported function:

```rust
use guestpy::prelude::*;
use guestpy::pyo3::CPython;

fn main() -> Result<(), Error> {
    let guest = Runtime::<CPython>::builder()
        .build()?
        .guest()
        .build()?;

    guest.exec(
        r#"
def add(left, right):
    return left + right
"#,
    )?;

    let sum = guest
        .globals()?
        .item::<Function<_>, _>("add")?
        .call::<_, i64>((20, 22))?;

    assert_eq!(sum, 42);

    Ok(())
}
```

`Runtime::<CPython>::builder()` selects CPython. Replace `CPython` with
`guestpy::rustpython::RustPython` to use RustPython. The calls in this example are synchronous;
GuestPy converts the arguments and result at the boundary.

## Runtimes, guests, and isolation

A `Runtime<B>` owns the selected backend. Each call to `Runtime::guest` builds a fresh, isolated
guest:

```rust
use guestpy::prelude::*;
use guestpy::pyo3::CPython;

let runtime = Runtime::<CPython>::builder()
    .build()?;
let first = runtime.guest().build()?;
let second = runtime.guest().build()?;

first.exec("name = 'first'")?;
second.exec("name = 'second'")?;

assert_eq!(first.globals()?.item::<String, _>("name")?, "first");
assert_eq!(second.globals()?.item::<String, _>("name")?, "second");
```

Bind host libraries, native libraries, and source bundles to a runtime when every guest should receive
them. Bind them to a guest builder when a capability belongs to one guest only. A runtime or guest
builder can also deny imports by name.

### Execution control

Configure a timeout and a cancellation signal on the runtime when an application needs to stop guest
execution:

```rust
use guestpy::prelude::*;
use guestpy::pyo3::CPython;

let cancellation = Cancellation::new();
let runtime = Runtime::<CPython>::builder()
    .timeout(std::time::Duration::from_millis(50))
    .cancellation(cancellation.clone())
    .build()?;
let guest = runtime.guest().build()?;

assert!(matches!(
    guest.exec("while True: pass"),
    Err(Error::Timeout),
));

cancellation.cancel();

assert!(matches!(
    guest.eval::<i64>("1 + 1"),
    Err(Error::Cancelled),
));
```

Use the selected backend's configuration type with `RuntimeBuilder::config` when interpreter-specific
configuration is required.

## Loading guest code

`Guest::exec` runs Python statements directly, with no return value. `Guest::eval` evaluates a single
Python expression and converts its result:

```rust
use guestpy::prelude::*;
use guestpy::pyo3::CPython;

let guest = Runtime::<CPython>::builder()
    .build()?
    .guest()
    .build()?;

assert_eq!(guest.eval::<i64>("6 * 7")?, 42);
```

Use `Guest::guest_module(name, source)` to load a named Python module and work with its exports:

```rust
use guestpy::prelude::*;
use guestpy::pyo3::CPython;

let guest = Runtime::<CPython>::builder()
    .build()?
    .guest()
    .build()?;

let module = guest.guest_module(
    "dynamic",
    r#"
settings = {'prefix': 'hello'}

def greet(name):
    return f"{settings['prefix']} {name}"
"#,
)?;

assert_eq!(
    module.object("settings")?.get::<String>("prefix")?,
    "hello",
);
assert_eq!(
    module.function("greet")?.call::<_, String>(("Ada",))?,
    "hello Ada",
);
```

This dynamic handle API is useful when the module shape is not known until runtime.

### Loading several modules and packages together with Bundle

Use `Bundle` when guest code is a package or spans several modules:

```rust
use guestpy::prelude::*;
use guestpy::pyo3::CPython;

let bundle = Bundle::builder()
    .module(
        "app.main",
        r#"
from app.util import double

def run():
    return double(21)
"#,
    )
    .package("app", "")
    .module("app.util", "def double(value):\n    return value * 2\n")
    .build()?;

let guest = Runtime::<CPython>::builder()
    .build()?
    .guest()
    .build()?;

let module = guest.load(&bundle)?;

assert_eq!(module.function("run")?.call::<_, i64>(())?, 42);
```

Build a bundle from modules, packages, and optional data, then load it directly or bind it so guest
code can import it. The `embedded` and `tokio` features provide additional source-loading options when
an application needs them.

## Typed guest facades

When a guest module has a known interface, `guestpy::guest_module!` generates a typed Rust facade:

```rust
use guestpy::prelude::*;
use guestpy::pyo3::CPython;

guestpy::guest_module! {
    pub module Math {
        fn add(left: i64, right: i64) -> i64;

        value answer: i64;
    }
}

let guest = Runtime::<CPython>::builder()
    .build()?
    .guest()
    .build()?;

let math = Math::from(
    guest.guest_module(
        "math",
        r#"
def add(left, right):
    return left + right

answer = 42
"#,
    )?,
);

assert_eq!(math.add(20, 22)?, 42);
assert_eq!(math.answer()?, 42);
```

Declare the functions and values the host needs. The generated facade converts each result into the
descriptor type declared in the macro.

### Typed guest classes

Use `guestpy::guest_class!` when the host knows the interface of a Python class or instance:

```rust
use guestpy::prelude::*;
use guestpy::pyo3::CPython;

guestpy::guest_class! {
    pub class Client {
        fn get(path: String) -> Response<B>;

        value prefix: String;
    }
}

guestpy::guest_class! {
    pub class Response {
        fn status() -> i64;
    }
}

guestpy::guest_module! {
    pub module Plugin {
        #[guestpy(name = "Client")]
        value client_class: Class<B, Client<B>>;

        value default_client: Client<B>;
    }
}

let guest = Runtime::<CPython>::builder()
    .build()?
    .guest()
    .build()?;

let plugin = Plugin::from(
    guest.guest_module(
        "plugin",
        r#"
class Response:
    def __init__(self, code):
        self.code = code

    def status(self):
        return self.code

class Client:
    def __init__(self, prefix):
        self.prefix = prefix

    def get(self, path):
        return Response(len(self.prefix + path))

default_client = Client('default:')
"#,
    )?,
);

let client = plugin
    .client_class()?
    .construct(("api:".to_owned(),))?;

assert_eq!(client.prefix()?, "api:");
assert_eq!(client.get("users".to_owned())?.status()?, 9);
```

A `Class<B, R>` remembers the descriptor used to convert its constructed instance. A generated class facade wraps an `Instance<B>` and calls Python attributes normally, so the same facade works for Python-defined classes or host-injected classes. Use `Object<B>` for unrestricted dynamic access, `Instance<B>` for a dynamic instance view, and `Instance<B, C>` when the live object carries a checked host payload `C` that Rust must borrow.

## Guest-side async

Calling a Python `async def` function returns a coroutine object; converting it to a `Coroutine<B, T>`
or `Awaitable<B, T>` and awaiting that value from an async Rust function drives the guest's own asyncio
event loop until the coroutine completes, and produces the awaited Python return value as a `T`:

```rust
use guestpy::prelude::*;
use guestpy::pyo3::CPython;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let guest = Runtime::<CPython>::builder()
        .build()?
        .guest()
        .build()?;

    guest.exec(
        r#"
import asyncio

async def double(value):
    await asyncio.sleep(0)
    return value * 2
"#,
    )?;

    let doubled = guest
        .globals()?
        .item::<Function<_>, _>("double")?
        .call::<_, Coroutine<_, i64>>((21,))?
        .await?;

    assert_eq!(doubled, 42);

    Ok(())
}
```

Use `Coroutine<B, T>` when the result must be a coroutine. Use `Awaitable<B, T>` when guest code may
return either a direct value or an awaitable. Other host and guest operations are synchronous.

## Plain Rust data

Derive `ToGuest` and `FromGuest` for ordinary serde-compatible Rust structs and enums under the `serde`
Cargo feature. These derives delegate the entire conversion to the type's own
`serde::Serialize`/`serde::de::DeserializeOwned` implementation, so ordinary `#[serde(...)]` field and
variant attributes define the exact Python-side representation:

```toml
[dependencies]
serde = { version = "1", features = ["derive"] }
```

```rust
use guestpy::prelude::*;

#[derive(
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    guestpy::ToGuest,
    guestpy::FromGuest,
)]
struct Request {
    #[serde(rename = "userId")]
    user_id: u64,
    note: Option<String>,
}
```

The derives follow the type's serde representation. Standard Rust primitives, options, collections,
arrays, tuples, and iterable values also cross the boundary directly. Use a host class when guest
code needs to retain Rust object identity rather than receive copied data.

## Host classes

`#[guestpy::host_class]` exposes an ordinary Rust type as a Python class. Mark the constructor,
methods, and properties guest code needs:

```rust
use guestpy::prelude::*;

struct Vector2 {
    x: f64,
    y: f64,
}

#[guestpy::host_class]
impl Vector2 {
    #[guestpy(constructor)]
    fn new(x: f64, y: f64) -> Result<Self, Error> {
        Ok(Self { x, y })
    }

    #[guestpy(method)]
    fn length(&self) -> Result<f64, Error> {
        Ok(self.x.hypot(self.y))
    }

    #[guestpy(get)]
    fn x(&self) -> Result<f64, Error> {
        Ok(self.x)
    }
}
```

Guest code sees the generated class exactly as if it were written in Python:

```python
vector = Vector2(3, 4)
print(vector.x)
print(vector.length())
```

Returning a host-class value to Rust preserves the live guest instance rather than cloning the Rust payload. Use `Instance<B, Vector2>::borrow_with` or `borrow_with_mut` when the host needs direct payload access; ordinary facade calls still use Python dispatch.

The macro also supports mutable methods, class-level members, Python protocol methods, inheritance,
and asynchronous host work. Refer to the API documentation when one of those capabilities is needed.

## Host modules

`#[guestpy::host_module]` exposes Rust functions and classes through a Python-importable module:

```rust
use guestpy::prelude::*;
use guestpy::pyo3::CPython;

struct Geometry;

#[guestpy::host_module(name = "host_geometry")]
impl Geometry {
    #[guestpy(function)]
    fn hypot(left: f64, right: f64) -> Result<f64, Error> {
        Ok(left.hypot(right))
    }
}

let runtime = Runtime::<CPython>::builder()
    .bind(Geometry.module())
    .build()?;
```

Guest Python code imports the generated exports as ordinary module attributes:

```python
import host_geometry

print(host_geometry.hypot(5, 12))
```

The macro can also expose values, getters, nested objects, initialization, and guest-visible
exceptions. Bind one module directly to a runtime or guest builder according to the scope that should
receive it. Use `HostLibrary` when that scope needs more than one host module.

## Native libraries

Native libraries are the backend-specific escape hatch for a module that cannot be expressed as a host
module. Prefer `#[guestpy::host_module]` for normal integrations. For CPython, build a native pyo3
module and bind it directly. This advanced example also needs a direct `pyo3` dependency because it
constructs pyo3 values itself:

```rust
use std::rc::Rc;

use guestpy::prelude::*;
use guestpy::pyo3::CPython;
use pyo3::{Python, types::PyModule};

let native = NativeModule::<_>::new(
    "host_environment",
    Rc::new(|py: Python<'_>| {
        let module = PyModule::new(py, "host_environment")?;

        module.add("runtime", "guestpy")?;

        Ok(module.into())
    }),
);

let runtime = Runtime::<CPython>::builder()
    .bind_native(native)
    .build()?;
```

Bind one native module directly to a runtime or guest builder. Use `NativeLibrary` when that scope
needs more than one native module or initializer.

## Errors

Every GuestPy operation returns `Result<T, guestpy::Error>`. Inspect a guest exception when Python
code fails, and use normal Rust error handling for engine, conversion, import, and execution-control
failures. Match a known Python exception when the host can recover from it, and propagate every other
error:

```rust
use guestpy::prelude::*;
use guestpy::pyo3::CPython;

fn read_count(guest: &Guest<CPython>) -> Result<Option<i64>, Error> {
    match guest.eval::<i64>("int('not a number')") {
        Ok(count) => Ok(Some(count)),
        Err(Error::Guest(exception)) if exception.matches("ValueError") => {
            eprintln!(
                "guest rejected the count: {}",
                exception.message(),
            );

            Ok(None)
        }
        Err(error) => Err(error),
    }
}
```

`GuestException::matches` recognizes the exception's Python inheritance hierarchy. Use its
`qualified_name`, `message`, and optional `traceback` when reporting a guest failure. Host callables
can return application errors that convert into `guestpy::Error`:

```rust
#[derive(Debug, thiserror::Error)]
#[error("geometry operation failed")]
struct GeometryError;

impl From<GeometryError> for guestpy::Error {
    fn from(error: GeometryError) -> Self {
        guestpy::Error::sourced_unexpected(error.to_string(), error)
    }
}
```

## License

GuestPy is licensed under the ISC License. See [LICENSE](LICENSE).

## Support and feedback

Report problems and feature requests through the
[repository issue tracker](https://github.com/wizrds/guestpy/issues).
