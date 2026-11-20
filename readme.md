# Veto (`vetoc`) v0.1

> **Нет математической гарантии — нет бинарника.**  
> Системный компилируемый язык программирования со строгими паспортными диапазонами переменных (`i32<min..max>`), проверкой границ массивов на этапе компиляции и встроенным ИИ-Инквизитором (`prove(...)`).

---

## Установка и сборка

### На Windows (через MSYS2 MinGW64)
1. Установите **Rust** (`rustup-init.exe` с `rustup.rs`, выбрав тулчейн `x86_64-pc-windows-gnu`).
2. Установите **MSYS2** (`msys2.org`), откройте терминал **MSYS2 MINGW64** и выполните:
   ```bash
   pacman -Syu
   pacman -S mingw-w64-x86_64-gcc mingw-w64-x86_64-SDL2
   ```
3. Добавьте `C:\msys64\mingw64\bin` в системную переменную `PATH`.
4. Соберите и запустите свою программу (например, `my_game.vt`):
   ```powershell
   cargo run --features inquisitor -- my_game.vt
   .\veto_app.exe
   ```

### На Linux (Ubuntu / Debian)
```bash
sudo apt update && sudo apt install -y build-essential libsdl2-dev cargo rustc
cargo run --features inquisitor -- hello.vt && ./veto_app
```

---

## Интерактивный суд ИИ-Инквизитора

Если компилятор не может автоматически вывести доказательство для `prove(...)`, откройте суд с локальной нейросетью (`MiniModel`):
* **Linux:** `VETOC_ALLOW_MANUAL=1 cargo run --features inquisitor -- my_code.vt`
* **Windows (PowerShell):** `$env:VETOC_ALLOW_MANUAL="1"; cargo run --features inquisitor -- my_code.vt`

---

## Справочник синтаксиса Veto (для написания своих программ)

### Типы и функции с контрактами
* Каждая переменная и аргумент объявляются со строгим диапазоном: `let mut x: i32<0..800> = 400;`
* Функции проверяют аргументы и возвращаемое значение:
  ```veto
  fn clamp(v: i32<-1000..1000>) -> i32<0..100> {
      if v < 0 { return 0; }
      if v > 100 { return 100; }
      return v;
  }
  ```
* **Массивы:** `let mut buf: [i32<0..255>; 64] = [0; 64];` — при чтении и записи `buf[i]` компилятор гарантирует `0 <= i < 64`.
* **Управляющие конструкции:** `if / else`, `while cond { ... }`, `loop { ... }`, `prove(условие);`.
* **Операторы:** `+`, `-`, `*`, `/`, `%`, `<<`, `>>`, `&`, `|`, `<`, `<=`, `>`, `>=`, `==`, `!=`.

### Встроенные функции графики и системы (SDL2 Runtime)
* `gfx_open(w: i32<100..1920>, h: i32<100..1080>) -> i32<0..1>` — открывает окно `w x h`.
* `gfx_key_down(code: i32<1..5>) -> i32<0..1>` — проверяет нажатие клавиши (`1` = W/Y, `2` = A/G, `3` = S/H, `4` = D/J, `5` = Space; `Esc` закрывает окно).
* `gfx_clear() -> i32<0..0>` — очищает экран и Z-буфер.
* `gfx_draw_vline(col: i32<0..399>, top: i32<0..600>, bot: i32<0..600>, shade: i32<0..255>, dist: i32<1..64>)` — рисует вертикальную полосу шириной 2px с записью глубины.
* `gfx_draw_enemy(screen_x: i32<0..800>, dist: i32<1..64>, hurt_flash: i32<0..10>)` — рисует спрайт с учётом Z-буфера.
* `gfx_draw_gun(recoil: i32<0..10>, kills: i32<0..100>)` — рисует прицел, оружие и счётчик фрагов.
* `gfx_present()` — выводит кадр на экран (~60 FPS).
* `veto_rand(max_val: i32<1..1000>) -> i32<0..1000>` — случайное число от `0` до `max_val`.