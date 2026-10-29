# Flint 0.1.69 — фактическое состояние

> **DEV BUILD. НЕ АУДИРОВАН. НЕ ДЛЯ РЕАЛЬНЫХ СРЕДСТВ.**
> Сеть по умолчанию — testnet. Mainnet-транзакции не отправлялись.

## Оболочка

- **Tauri удалён 2026-10-26**; единственный UI — `crates/slint-ui` (`flint-slint`).
- Актуальный portable-пакет: `lastver/Flint-0.1.69-keyboard/`; манифест —
  `lastver/BUILD-INFO.json`. Release exe SHA-256 `f311fe52…`, selftest обновлён.
- Установленная копия **`C:\Flint`** обновлена вручную: текущие exe, selftest,
  `resources/`, README и `BUILD-INFO.json`. Каталоги `C:\Flint\data` и
  `C:\Flint\data1` сохранены; в `data1` есть mainnet-данные, они не изменялись.
- Старые `data-root-debug`/`slint-preview` пакеты, zip/source и промежуточные
  `.bak` удалены. В текущем пакете оставлен один rollback-exe
  `Flint.exe.bak-20261028-0852`.
- Backup Tauri-кода: `archive/archV/old-builds/tauri/tauri-removal-20261026-091749/`.
- RPC, Tor, messenger и ключ истории — в `crates/runtime`.
- Контрольный снимок: `20261029-plan12` — **320 passed, 0 failed,
  31 ignored**, UI-тесты — 21 passed. Снимок: 696 файлов, 62 517 489 байт,
  `match:true`; source SHA-256 `71bbb77c…`.
- Portable RC: `dist/Flint-0.1.69-slint-preview.zip`, exe `a61557b5…`,
  source `6833c2f4…`, unsigned; SBOM/источники/контракт включены.
- **GitHub CI/live jobs выполнены:** build-test **PASS**
  (run 40799284192); live-funded-testnet **PASS** (run 40811329947,
  embedded + RPC full-cycle, живой testnet); live-tor **PASS**
  (run 40813171901). Локальный self-hosted runner использовался только
  для live-джоб.
- **Статус: READY_FOR_EXTERNAL_DEVICE_TESTS.** Автоматические и локальные
  live-проверки закрыты; остаются только физические проверки из
  `DEVICE_ACCEPTANCE.md`.
  генерирует `.po` и `src/i18n_catalog.rs`, `-Check` входит в CI.
- `main.rs` разделён дальше: `format.rs`, `presentation.rs`, `sanitize.rs`,
  `platform.rs`, `tor_node.rs`, `settings_handlers.rs`, `onboarding_handlers.rs`, `messenger_handlers.rs`, `activity_handlers.rs`; main — 2098 строк.
  Release-обзор и открытие настроек после разделения — **PASS**.
- Tor live: **PASS** (direct_onion 1, tor_hidden_service 2, tor_listener_gateway 1; relay_live 1).
- В CI добавлены doctests, rustdoc и проверка `.po`; desktop workflow не
  выполняет живые Tor/testnet jobs без явного dispatch.
- Live embedded transaction lifecycle: **PASS** (полный цикл, отмена,
  рестарт). RPC live на отдельных RPC-кошельках — **PASS**. GitHub live jobs
  не запускались.
- Ручная проверка release UI: обзор, история/`response_received`, экспорт
  ответа (1902 символа, `BEGINSLATEPACK…ENDSLATEPACK`), сообщения, настройки,
  отправка и масштаб **125%** — **PASS**; кадры
  `.tmp/manual-ui-{wallet,history,messages,settings,send,scale125}.png`.
  Масштаб **150%**: окно 1920×1290, onboarding и диалоги создания/восстановления — **PASS**.

## Проверено

> Числа и снимки в исторических пунктах ниже относятся к указанным датам. Текущий
> контрольный прогон — **323 passed, 0 failed, 31 ignored**, UI — 21 passed,
> снимок `20261029-plan12`.

- В «Новый кошелёк» поля пароля/повтора сужены до 360 px, карточка на шаге
  пароля — 430 px (seed — 650 px), контент выровнен по верху. UI-тесты — 14
  passed; скриншот `docs/assets/shots/flint-create-wallet.png`. Логика пароля/seed
  не менялась.
- Чекбокс «Обрабатывать слейтпаки автоматически» в сообщениях: по умолчанию
  выключен; при включении обрабатывает только armor от сохранённых контактов —
  `prepare_receive` → `finalize_receive` → автоотправка response; для ответа на
  отправку ищет pending send и вызывает `finalize_send`. Добавлен фоновый
  inbox-опрос раз в 10 с при включённом чекбоксе и обрабатывается весь батч
  armor-сообщений, а не только первое. UI-восстановление старой истории проверено
  на тестовом legacy-каталоге («старая история восстановлена», ключ истории
  открыт). Реальный encrypted slatepack прогнан: default → alice, 1 GRIN, без
  перезапуска приложения, смена профиля через dropdown; первый входящий на alice
  обработан фоновым таймером без нажатия кнопки — диагностика зафиксировала
  `prepare ok` → `finalize_receive ok` → `response sent` для реального slate на
  0.1 GRIN. На отдельном прогоне 1 GRIN response был восстановлен идемпотентным
  `prepare_receive`, default выполнил `finalize_send`/`post_tx`, обе стороны
  получили `confirmed`: alice 1.000000000, default 1397.390000000 GRIN
  (change сходится: 1398.413 − 1.0 − 0.023). Реальный батч из двух настоящих
  armor (0.05 и 0.002 GRIN) обработан одним auto-циклом: два receive-pending с
  `response_received`; доставка/финализация обоих ответов не проверена. Чекбокс
  автофинализации сохраняется в `<data>/auto-slatepacks` и переживает рестарт.
  UI-тесты — 15 passed; полный набор — **313
  passed, 0 failed, 29 ignored**. Скриншот:
  `docs/assets/shots/flint-slatepack-real.png`.
- Диагностика node-secret: ошибка owner API 401/403 теперь отображается как
  «нода отклонила API-секрет», таймаут/HTTP — отдельно; `NodeInfo.status_error`
  сохраняет причину. Тест с неверным секретом — passed.
- Slatepack-детектор в сообщениях: armor `BEGINSLATEPACK. … ENDSLATEPACK.`
  распознаётся, в бабле появляется «Обработать slatepack», открывающий диалог
  получения с подготовленным armor. Авто-финализация выключена по умолчанию и
  включается чекбоксом только для сохранённых контактов; ручной путь — «Принять» →
  проверка суммы → «Завершить приём». Unit-тест armor и UI-действие проверены;
  скриншот `docs/assets/shots/flint-slatepack-detect.png`. Полный набор — **313
  passed, 0 failed, 29 ignored**; UI-тесты — 15 passed. Подробности — записи 46/48
  в [WORK_LOG.md](../WORK_LOG.md).
- Автообработка slatepack: при ошибке signed-ack входящие всё равно возвращаются
  в UI (`ack_error`), поэтому автофинализация не пропускает первый ответ;
  входящий armor хранится в durable queue (`received/processing/failed/done`),
  crash/restart drill: файл пережил `Stop-Process -Force` и был обработан после
  повторного запуска (`queue cleared`);
  исходящий envelope сохраняется в `<data>/messenger/outbox/...` до доставки и
  повторяется с тем же id после сбоя/перезапуска. Смена пароля очищает
  пятиминутный unlock-кэш, в UI доступный/ожидающий/заблокированный баланс
  разделены, очередь переводов показывает next-action, диагностика копируется
  без секретов. Подробности — запись 65. Ручная проверка нового интерфейса:
  pending `response_received` и кнопка «Экспорт ответа» проверены вживую на
  `ui-rustsec-live`; clipboard начинается `BEGINSLATEPACK.` и заканчивается
  `ENDSLATEPACK.`, pending остался `response_received`. Запись 67.
  Восстановление funded testnet-кошелька в отдельном каталоге — **PASS**:
  после полного скана `scan_state=completed`, баланс проверен, доступно
  **1397.390000000 GRIN**, адрес совпал с прежним. Запись 68.
- Messenger E2E на двух testnet-профилях с настоящим loopback `flint-relay`:
  отправка/приём, inbox, история с направлениями, рестарт, неверный профиль и пароль —
  **3 passed** для embedded и RPC. Live UI: `default → alice` и `alice → default`,
  «Проверить входящие», структурированная история; скриншот
  `docs/assets/shots/flint-messenger-live.png`. В UI проверены блокировка/
  разблокировка контакта (найден и исправлен обрезанный «Разблокировать»).
  Добавлен тест успешного восстановления старого `.history.key` — passed.
  Полный набор — **304 passed, 0 failed, 29 ignored**; UI-тесты — 12 passed.
  Подробности — запись 45 в [WORK_LOG.md](../WORK_LOG.md).
- Lock-screen перестроен: центрированная карточка 510 px с высотой по содержимому,
  знак/имя/фактическая сеть, «Открыть кошелёк» и статус с замком, один переключатель
  профиля, пароль с подсказкой и зарезервированной ошибкой, широкая мятная
  «Разблокировать» с «Открываю…», нижние действия и footer. Проверены 1280×860,
  900×760, 125%/150% (`SLINT_SCALE_FACTOR`), ненастроенный кошелёк, переключение
  профиля, неверный пароль, длинное имя, busy-состояние и автофокус пароля.
  UI-тесты — 12 passed; полный набор **303 passed, 0 failed, 29 ignored**.
  Подробности — запись 44 в [WORK_LOG.md](../WORK_LOG.md).
- Бренд **Flint** и «Сообщения»: знак A «Складка» (SVG + PNG 16/24/32/48/64/128,
  ico), графитовая тема с мятой, минимум окна **900×760**. Сообщения — две области:
  контакты/блокировки слева, переписка и composer справа; своих адрес короткий с
  раскрытием и копированием; диалоги контакта/восстановления; структурированные
  строки истории с направлением и временем. Проверены пустое состояние, контакт,
  ошибка адреса, восстановление без каталога (пароль очищен), 900×760. UI-тесты —
  12 passed; полный набор **303 passed, 0 failed, 29 ignored**. Подробности —
  запись 43 в [WORK_LOG.md](../WORK_LOG.md); скриншоты — `docs/assets/shots/flint-*`.
- Дизайн главного экрана завершён: бренд в сайдбаре, один переключатель профиля,
  раздельные карточки баланса/получения, панель ноды, пустая история с контурной
  иллюстрацией и ссылкой, testnet-бейдж и подпись «Тестовая сеть». Кнопки получили
  иконки и приоритет (primary/secondary/ghost), состояния hover, phase-статус ноды
  («статус ноды неизвестен»/«нода готова»). Проверено скриншотами. UI-тесты — 12
  passed; полный набор **303 passed, 0 failed, 29 ignored**; снимок
  `20261028-ui-final`: 673 файла, 62 380 445 байт, `match:true`. Подробности —
  запись 42 в [WORK_LOG.md](../WORK_LOG.md).
- Главный экран перестроен по макету: шапка «Обзор кошелька» с профилем и testnet-бейджем,
  баланс отдельной карточкой с суммой/GRIN и статусом, справа — карточка получения с
  коротким адресом, копированием и QR-модалкой, ниже — отдельная панель ноды с
  «Подробнее» и сканированием, затем история с дружелюбным пустым состоянием.
  Профиль в сайдбаре — один переключатель с выпадающим списком; меню с иконками.
  Проверено скриншотами. UI-тесты — 12 passed; полный набор **303 passed, 0 failed,
  29 ignored**; снимок `20261028-ui-redesign`: 673 файла, 62 376 741 байт,
  `match:true`. Подробности — запись 41 в [WORK_LOG.md](../WORK_LOG.md).
- Профили и меню оформлены: последние профили — карточки с аватаром и подписью,
  активная подсвечена мятным; полный список — строка компактных чипов вместо
  ComboBox; пункты меню — кнопки с иконкой, высотой 46 px и акцентной полосой.
  Проверено скриншотами lock-экрана и разблокированного кошелька. UI-тесты — 12
  passed; полный набор **303 passed, 0 failed, 29 ignored**; снимок
  `20261028-ui-tiles`: 673 файла, 62 359 957 байт, `match:true`. Подробности —
  запись 40 в [WORK_LOG.md](../WORK_LOG.md).
- UI-полировка: окно 1280×860, сайдбар 250 px, пункты меню 44 px/14 px с акцентной
  полосой, основные подписи увеличены (9→11, 10→12, 11→13, 18→20), суммы монет
  оставлены 32 px. Тёмная тема получила более мягкие поверхности/границы и радиусы.
  Ctrl+Z после вставки большого текста в пароль/seed восстанавливает значение до
  вставки: внешняя установка текста очищает нативный undo-стек Slint, поэтому для
  вставки хранится один шаг. Тесты UI — 12 passed; полный набор **303 passed,
  0 failed, 29 ignored**; снимок `20261028-ui-polish`: 673 файла, 62 357 266 байт,
  `match:true`. Подробности — запись 39 в [WORK_LOG.md](../WORK_LOG.md).
- Ctrl+Z/Ctrl+A: в полях пароля, seed-фразы, адреса получателя и входящего slate
  добавлены обработчики `undo`/`select-all`; в многострочном seed-поле Ctrl+A
  выделяет всё, Ctrl+Z остаётся нативным (у TextEdit нет публичного undo). Полный
  набор: **303 passed, 0 failed, 29 ignored**; снимок `20261028-undo-select`:
  673 файла, 62 355 411 байт, `match:true`. Подробности — запись 38 в
  [WORK_LOG.md](../WORK_LOG.md).
- Профили: сверху показываются до трёх последних открытых профилей крупными
  кнопками, ниже — полный список. Пароль успешной разблокировки хранится только в
  памяти 5 минут и переиспользуется при переключении в этом окне; после 5 минут
  системного бездействия (нет ввода мыши/клавиатуры) кошелёк блокируется, кэш
  очищается. Тест `unlock_grace_cache_and_recent_profile_round_trip`. Полный набор:
  **303 passed, 0 failed, 29 ignored**; снимок `20261028-profile-recents`: 673 файла,
  62 353 271 байт, `match:true`. Подробности — запись 37 в [WORK_LOG.md](../WORK_LOG.md).
- Ctrl+V: нативный Slint-путь на этой сборке не срабатывал без явного клика по полю.
  Добавлен резервный путь через `arboard`: callback `paste-text`, поля пароля/
  seed-фразы/адреса/входящего slate принимают Ctrl+V и вставляют текст из системного
  буфера; кнопка «Вставить» у пароля использует тот же путь. Проверено через UI
  Automation: клик по полю «Пароль» → Ctrl+V → в поле появился текст (маскированный).
  Полный набор: **302 passed, 0 failed, 29 ignored**; снимок `20261028-paste-fallback`:
  673 файла, 62 342 438 байт, `match:true`. Подробности — запись 36 в
  [WORK_LOG.md](../WORK_LOG.md).
- Показ seed: в разделе «Безопасность» и в настройках профиля добавлена кнопка
  «Показать seed». Она требует пароль, расшифровывает конверт и показывает фразу в
  read-only поле; пароль после успеха очищается, фраза не пишется в лог и не
  копируется автоматически, при закрытии/смене профиля стирается из UI. Ядро
  проверено тестом `seed::store::tests::reveal_mnemonic_round_trips_and_rejects_wrong_password`
  (верная фраза, неверный пароль). Полный набор: **302 passed, 0 failed, 29 ignored**;
  снимок `20261028-seed-reveal`: 673 файла, 62 340 351 байт, `match:true`.
  Подробности — запись 35 в [WORK_LOG.md](../WORK_LOG.md).
- Клавиатура: Enter отправляет основные формы — разблокировку, создание, восстановление,
  отправку, настройки, смену пароля и удаление. В поле пароля добавлена кнопка
  «Вставить», а кастомный перехват Ctrl+V удалён: вставку обрабатывает нативный
  Slint `LineEdit`/`TextEdit`. Полный набор: **301 passed, 0 failed, 29 ignored**;
  снимок `20261028-keyboard`: 673 файла, 62 332 623 байта, `match:true`. Подробности —
  запись 34 в [WORK_LOG.md](../WORK_LOG.md).
- Мультипрофили: в настройках появился раздел «Профили» с активным именем, полем и
  кнопкой «Создать профиль»; на экране блокировки — «Новый профиль». Новое имя
  переключает на отдельный профиль и сразу открывает создание seed; выбор пустого
  профиля в списке тоже открывает создание. Переименования нет: новое имя — это
  новый профиль. Изоляция seed/address/scan по профилям покрыта
  `embedded_profiles_keep_seeds_addresses_and_scan_state_isolated`; UI-валидация —
  `profile_creation_rejects_empty_and_active`. Полный набор: **301 passed, 0 failed,
  29 ignored**; снимок `20261028-profiles-ux`: 673 файла, 62 330 395 байт,
  `match:true`. Подробности — запись 33 в [WORK_LOG.md](../WORK_LOG.md).
- Динамическая локализация завершена: `crates/slint-ui/src/i18n.rs` переводит
  строки, собранные в Rust, — точные пары и шаблоны с `{}`/`{name}`, 264 записи.
  Через слой проходят статусы кошелька/ноды, скан, Tor/мосты, буфер, activity,
  pending-уведомления, ошибки `flint-core`/`flint-runtime`/`service.rs`/`config.rs`
  и диалог выбора каталога; 29 `accessible-label` обёрнуты в `@tr`. Непереведённая
  русская строка в EN-режиме заменяется английским generic-сообщением и пишется в
  лог, поэтому русский текст в UI не показывается. Тесты:
  `exact_and_template_translation`, `nested_error_is_translated`,
  `core_error_is_translated`, `english_never_leaks_cyrillic`; EN-прогон на старте
  не дал `i18n: missing English translation`. Полный набор: **300 passed, 0 failed,
  29 ignored**; снимок `20261028-i18n-full`: 673 файла, 62 317 194 байта,
  `match:true`. Подробности — запись 32 в [WORK_LOG.md](../WORK_LOG.md).
- Локализация ru/en статического UI и справка восстановления: 109 строк обёрнуты в
  `@tr`, английский каталог `lang/en_US/LC_MESSAGES/flint-slint.po` встроен в бинарь,
  в настройках есть «Русский / English», переключение **на лету** без перезапуска;
  язык хранится в `ui-language`. Справка — отдельный overlay с четырьмя абзацами. Тесты:
  `every_tr_literal_has_an_english_translation`, `english_catalog_is_non_empty`,
  `ui_language_round_trips_and_defaults_to_russian`; запуск с `ui-language=en` печатает
  `ui language: en_US selected`. Динамические сообщения закрыты слоем `i18n.rs`
  (см. выше); a11y-подписи переведены, но полный a11y-аудит — внешний PS-11. На
  момент статической локализации набор был **296 passed, 0 failed, 29 ignored**;
  снимок `20261028-i18n`: 672 файла, 62 273 939 байт, `match:true`. Все **12
  локальных пунктов закрыты**; 17 внешних приёмок по-прежнему открыты. Подробности —
  запись 30 в [WORK_LOG.md](../WORK_LOG.md).
- Уведомления о незавершённых/истекающих slate реализованы: `pending_alerts` даёт
  critical для `finalizing`/`finalized_not_posted`/`failed_recoverable`/`response_lost`
  и warning после 30 минут для `prepared`/`locked`/`receive_intent`/`response_received`;
  главный экран показывает баннер `pending-alert` и обновляет его раз в 60 с (плюс при
  загрузке очереди). Тесты: `pending_alerts_sort_critical_first_and_warn_only_after_30_minutes`,
  `fresh_or_finished_slates_do_not_alert`. TTL у pending-сделки нет — 30 минут это
  возрастной порог, не expiry. Полный набор: **293 passed, 0 failed, 29 ignored**;
  снимок `20261028-slate-alerts`: 671 файл, 62 253 783 байта, `match:true`. Остался
  1 пункт этапа 5 — локализация/справка. Подробности — запись 29 в
  [WORK_LOG.md](../WORK_LOG.md).
- Этап 2 и часть этапа 5 закрыты локально. Offline/flapping/битая цепь проверены на
  node API: недоступный и закрытый узел теряют `synced`, чужой `chain` и `Err` от
  `get_tip` не дают Ready; отправка и скан по-прежнему блокируются без ready-ноды
  (физическая порча `chain_data` не воспроизводилась). Большие истории:
  `large_history_is_bounded_and_lists_newest_first` (120 записей, лимит 100, страница 20 —
  prune и порядок); длительный скан: живой cancel на высоте 3 311 753 — 1 passed, 97,98 с.
  Фоновая нода: дизайн и гейты — [BACKGROUND_NODE.md](../architecture/BACKGROUND_NODE.md),
  реализация не начата. Полный набор: **291 passed, 0 failed, 29 ignored**; снимок
  `20261028-stage2-ux`: 671 файл, 62 245 990 байт, `match:true`. Осталось 2 пункта
  этапа 5 (уведомления о slate, локализация/справка). Подробности — запись 28 в
  [WORK_LOG.md](../WORK_LOG.md).
- Этапы 3 и 4 закрыты локально. Tor проверен с Unicode/пробелами и мостами: Windows-сборка
  Tor не создавала DataDirectory на Unicode-пути, поэтому для каталогов Tor используется
  8.3-короткий ASCII-путь, а `Log ... file` пишется без кавычек (живой Unicode-тест —
  1 passed, 24,90 с; живой address-derived onion — 1 passed). Relay: flood получает 503,
  квота одного получателя не блокирует другого, сбой storage даёт 500 без сообщения в
  памяти. ContactBook: stale lock старше 30 с восстанавливается, свежий не трогается.
  Node/RPC: hang ограничен таймаутом, RPC-ребёнок перезапускается и открывает тот же
  адрес, живой embedded scan cancel — 1 passed, 97,98 с; graceful shutdown и restart
  покрыты прежними node-тестами. Native hashes: `vcruntime140.dll` и `pt_config.json`
  закреплены (6 записей, `verify-native-assets.ps1` проходит). SBOM: 1141 cargo +
  6 native компонентов, `verify-sbom.ps1` сверяет hashes и identity F-04. RustSec:
  2 транзитивные уязвимости разобраны (curve25519-dalek, yaml-rust и zip закрыты
  обновлениями), 17 предупреждений —
  [RUSTSEC_2026-10-28.md](../security/RUSTSEC_2026-10-28.md); обновление
  зависимостей — отдельный трек [RUSTSEC_UPGRADES.md](../security/RUSTSEC_UPGRADES.md).
  ACL-проверки теперь
  передают Windows PowerShell корректный `PSModulePath`: под pwsh 7 модуль Security
  не автозагружался и owner-only ACL ошибочно падал. На момент этапа 3–4 полный набор:
  **287 passed, 0 failed, 29 ignored**; снимок `20261028-stage34-local`: 671 файл,
  62 239 827 байт, `match:true`. Подробности — запись 27 в
  [WORK_LOG.md](../WORK_LOG.md).
- Настройки показывают активный корень данных и позволяют выбрать абсолютный путь через
  обзор или вручную, вернуть стандартный путь и применить выбор после перезапуска.
  Селектор `data-root.json` хранится рядом с portable `Flint.exe`, не внутри старого `data`;
  `--data`/`FLINT_DATA_DIR` имеют приоритет. Кошельки и цепь не копируются автоматически.
  Повреждённый селектор не игнорируется. Regression: сохранение/сброс, Unicode-путь,
  отказ относительного пути/файла и повреждённого JSON. Снимок
  `20261028-data-root-selection`: 669 файлов, 62 218 833 байта, `match:true`;
  обычные проверки завершены раздельными командами из-за лимита 40 с: **277 passed,
  0 failed, 28 ignored**. Debug `flint-slint` собран, selftest `ok:true`,
  `reopened_matches:true`; интерактивный UI новой настройки не проверен. Подробности —
  запись 24 в [WORK_LOG.md](../WORK_LOG.md).
- Полный restore lifecycle прошёл на funded testnet для embedded и реального RPC:
  восстановление из фразы в чистый каталог, отказ partial scan, полный chain-scan и
  reopen после restart. Независимый от original оракул на противоположном backend
  подтвердил адрес и точное совпадение `spendable`, `awaiting`, `total`, `immature`,
  `locked`; баланс составил 100 000 000 nano = 0,1 GRIN. Адрес также вычислен напрямую
  из entropy. Контрольный снимок `20261028-restore-lifecycle-live`: **669 файлов,
  62 199 866 байтов**, **274 passed, 0 failed, 28 ignored**, selftest `ok:true`,
  `match:true`. Оракул использует отдельный backend и scan, но ту же testnet-ноду.
  Живой RPC restore/full-scan и явный отказ partial scan закрывают PT-05. Это не mainnet
  restore и не закрытие PS-16. Подробности — записи 21–22 в
  [WORK_LOG.md](../WORK_LOG.md).
- Мультикошельки: legacy/default сохраняет прежний путь; именованные профили имеют
  изолированные wallet/state/pending/RPC/messenger/history-пути. `WalletRuntime`
  пересоздаёт wallet/node/RPC-контекст при переключении, Slint предлагает выбор
  кошелька на главном и lock screen, очищает секреты и transient state, отбрасывает
  asynchronous-результаты прежнего `wallet_generation` и блокирует UI-смену во время
  операций. CLI-сигнатуры сохранены. Embedded regression и живой RPC smoke
  create/switch/reopen двух независимых seed прошли. Живые embedded/RPC-сценарии через
  реальный `flint-relay` подтвердили обмен в обе стороны, изоляцию истории, restart,
  повторный inbox без дублей и отказ неверного профиля/пароля. Полный набор на снимке
  `20261027-multiwallet-messenger-live`: 269 passed, 0 failed, 22 ignored. Интерактивный
  UI не проверен. Барьер UI не является общим доказательством потокобезопасности
  внешних вызовов runtime.
- Повторяемый lifecycle `create → lock → open → restart → open → restore → delete`
  прошёл по два независимых цикла на чистых testnet-каталогах для embedded и реального
  RPC `grin-wallet`. После restart и restore адрес совпадает с исходным, восстановленный
  seed имеет вид `envelope` и требует scan, удаление перемещает кошелёк в `trash`.
  Полный набор на снимке `20261027-wallet-lifecycle-live`: **270 passed, 0 failed,
  23 ignored**. Это адресный lifecycle без полного chain-scan и сверки баланса; общий
  restore lifecycle и этап 2 этим не закрыты. Подробности — запись 18 в
  [WORK_LOG.md](../WORK_LOG.md).
- Seed create/migrate/disable/cleanup проверен при конкуренции и restart. Два
  независимых `WalletRuntime` на одном чистом корне для embedded и реального RPC дают
  ровно одного победителя create; после restart открывается адрес победителя. Общий
  `SeedStore` допускает ровно одну успешную конкурентную миграцию или отключение конверта,
  а гонка cleanup/disable не удаляет последний recoverable seed. Все шесть process-abort
  точек M0–M5 миграции восстанавливаются; создание после потери upstream `db/` открывает
  прежний адрес из envelope. Полный набор на `20261027-seed-concurrency-live`:
  **274 passed, 0 failed, 24 ignored**. Это deterministic crash injection, не физический
  power-loss. Подробности — запись 19 в [WORK_LOG.md](../WORK_LOG.md).
- Полный transaction lifecycle прошёл на funded testnet для embedded и реального RPC:
  полный scan, `prepare_send → prepare_receive → finalize_send/post → confirmation`,
  вторая отправка и `cancel_pending`. Каждая подтверждённая передача составила
  100 000 000 nano = 0,1 GRIN; после cancel `spendable` и `locked` точно восстановлены
  и повторно проверены после restart обоих кошельков. Контрольный снимок
  `20261027-transaction-lifecycle-live-v2`: **668 файлов, 62 178 912 байтов**,
  **274 passed, 0 failed, 26 ignored**, selftest `ok:true`, `match:true`. Общий restore
  lifecycle и этап 2 этим не закрыты. Подробности — запись 20 в
  [WORK_LOG.md](../WORK_LOG.md).
- F-04/C funded testnet: все четыре сценария embedded/RPC × `upstream`/`journal`
  прошли реальный exit 9, restart/retry, возврат канонически того же S2 без дублей,
  sender finalize/post. RPC-транзакции подтверждены в цепи, change spendable;
  embedded-результат дополнительно учтён полным RPC scan. Доказательства —
  `.tmp/crash-receive/20261027-073435-embedded-d5587ebc` и
  `.tmp/crash-receive/20261027-074132-rpc-db56a3c2`. Disk-full/power-loss не
  выполнялись; общий F-04 остаётся открытым до destructible-гейта.
- F-01 funded RPC crash/recovery исправлен и подтверждён в цепи. Найдена потеря
  подписи на границе Owner RPC: компактный `get_stored_tx` с пустым `sigs`
  реконструировал нулевые kernel excess/signature. Recovery/retry/republish теперь
  читают исходную `.grintx` и публикуют полный `Transaction` напрямую в настроенную
  ноду; нулевая подпись отклоняется. Повторная подпись не выполняется.
  `f8af12ab-…` подтверждён в блоке 3310555; исправленный recovery `dba5c703-…` —
  в 3310568; финальный chain-gated прогон `dbfe2160-…` — в 3310581. Crash-script
  теперь не выдаёт PASS без on-chain kernel. Лог —
  `.tmp/f01-funded-rpc-crash-chain-gate.log`.
- Для F-04 подготовлены [destructible runbook](../operations/DESTRUCTIBLE_F04.md),
  фазовый harness и feature-gated pause-hook. По решению владельца вместо VM выполнена
  принятая симуляция DF-J: точечный `ERROR_DISK_FULL=112` после upstream commit и до
  response journal. Embedded/RPC восстановили тот же S2 с одним `TxReceived`; kernels
  подтверждены в блоках 3310716 и 3310738. Для embedded ожидание внешней командой
  истекло, поэтому его chain-подтверждение выполнено отдельным запросом, без новой
  сделки. Power-loss представлен прежними exit 9/restart. Это fault injection, не
  физические аварии и не формально закрытый destructible-гейт; дальнейшие VM-испытания
  по решению владельца прекращены. Workspace: **269 passed, 0 failed, 20 ignored**;
  guards 8, pause unit 2. Снимок `20261027-f04-simulated-disk-full`: 659 файлов,
  `match:true`. Подробности — запись 14 в [WORK_LOG.md](../WORK_LOG.md).
- Контрольный снимок `20261027-f01-raw-stored-recovery`: **269 passed,
  0 failed, 20 ignored**; fmt, workspace check, release Slint build и selftest успешны;
  656 файлов, `match:true`. Логи — `.tmp/f01-fix-*`. Подробности — запись 12 в
  [WORK_LOG.md](../WORK_LOG.md).
- F-04/A/B реализованы в `third_party/grin-wallet` и Flint по
  [ADR](../security/ADR_F04_RECEIVE_RECOVERY.md). Commit
  `11046cd197f8296e6cd9313502a9f2cbce6e262a` на базе `grin-wallet` 5.5.1 сохраняет
  recipient output, `TxReceived`, исходный S1 и подписанный S2 одним LMDB-commit.
  Точный повтор возвращает сохранённый S2; изменённый S1, повреждение recovery и
  legacy `TxReceived` без него отклоняются без повторной подписи. Embedded и
  управляемый RPC собираются из этого fork; внешний RPC без
  `receive_response_recovery >= 1` отклоняется до `receive_tx`. Сохранённый Flint
  response armorится без повторной capability-проверки, старый `ResponseLost`
  автоматически не восстанавливается. Typed recovery errors ведут в `ResponseLost`.
  RPC binary SHA-256: `c39c6a59bec0620c99b972c7a951326cbd20abee37de0221e0ce902184904e84`.
  Source package содержит 545 проверенных файлов fork/submodule, patch и offline
  config; SBOM записывает commits и SHA-256 source manifest.
- Контрольный снимок `20261027-f04-integration-candidate-v3`: **267 passed, 0 failed,
  19 ignored**; fmt, workspace check, debug Slint build и selftest успешны, sentinel
  не изменён; native hashes, source package, SBOM и `match:true` snapshot verify
  проверены. Upstream libwallet: **38 passed**; API: **94 passed**; transaction:
  **5 passed**; invoice: **1 passed**. Это unit/in-process fault-injection и
  упаковочные проверки, не OS crash/power-loss или живой RPC receive. Живой этап C
  выполнен позже; физический destructible-гейт PS-03 остаётся открытым. Логи — `.tmp/f04-*`.
- Снимок `20261027-f01-r01-candidate`: во всех трёх RPC-путях finalize и в embedded
  любая ошибка `finalize_tx` теперь ведёт в `FailedRecoverable`; текст ошибки больше
  не создаёт `FinalizedNotPosted`, повторная подпись и auto-publish остаются запрещены.
  Обычная ошибка receive сохраняет `ReceiveIntent`; на этом снимке `ResponseLost`
  назначался при явном `TransactionAlreadyReceived` (typed recovery errors добавлены
  позже в F-04/B). Ошибки журнала не принимаются за отсутствие
  записи. `cargo fmt`, workspace check и debug Slint build — успешно;
  `cargo test --workspace --locked --offline` — **265 passed, 0 failed, 19 ignored**;
  selftest — `ok:true`, sentinel не изменён, временный каталог удалён. SHA-256 дерева
  после проверок совпадает со снимком. Живой crash-прогон не повторён; F-04 остаётся
  открытым. Логи — `.tmp/f01-r01-*`.
- Снимок `20261027-sent01-candidate-v2`: добавлен `write_private_atomic` —
  owner-only ACL применяется к пустому уникальному temp до записи, затем выполняются
  flush, durable replace и повторная проверка final ACL. Подавление ошибок устранено
  для node/RPC/onion секретов, pending/archive, history key, blocked/settings и
  RPC-журналов; `.owner_api_secret` проверяется до чтения, при сбое дочерний
  `grin-wallet` завершается. Fault-injection покрывает ACL до записи, после rename
  и при открытии. `cargo fmt`, workspace check и debug Slint build — успешно;
  `cargo test --workspace --locked --offline` — **259 passed, 0 failed, 19 ignored**;
  selftest — `ok:true`, sentinel не изменён, временный каталог удалён. SHA-256
  дерева после проверок совпадает со снимком. Логи — `.tmp/sent01-v2-*`.
- Снимок `20261027-sent02-candidate`: `win_acl::restrict` заменяет DACL и
  перечитывает allowlist; regression с `Everyone:F`, пробелами/Unicode и повторным
  вызовом прошёл. `cargo fmt`, workspace check и debug Slint build — успешно;
  `cargo test --workspace --locked --offline` — **257 passed, 0 failed, 19 ignored**;
  selftest — `ok:true`, sentinel и временные данные проверены. SHA-256 дерева после
  проверок совпадает со снимком. Логи — `.tmp/sent02-*`.
- Снимок `20261027-024832-candidate`: `cargo fmt --all -- --check` — чисто;
  `cargo check --workspace --all-targets --locked --offline` — успешно;
  `cargo test --workspace --locked --offline` — **256 passed, 0 failed, 19 ignored**.
  SHA-256 сверка после проверок: дерево совпадает со снимком. Подробности и логи —
  [../WORK_LOG.md](../WORK_LOG.md).
- `restarts_cleanly_after_a_normal_stop` прошёл в полном наборе; node_manager:
  **3 passed**. Прежний незавершённый запуск не воспроизвёлся.
- Debug `cargo build -p flint-slint --locked --offline` — успешно.
- `flint-selftest` на том же снимке — `ok:true`; заданный `FLINT_DATA_DIR` не
  изменён, собственный временный каталог удалён. Selftest из `C:\Flint` ранее —
  `ok:true`.
- Live RPC password change (`--ignored`, реальный `grin-wallet`):
  legacy и envelope; старый пароль отклонён после lock/restart, новый
  открывает кошелёк, адрес сохраняется.
- Package: `lastver/Flint-0.1.69-slint-preview.zip`,
  **45 259 212 байт**, SHA-256
  `8A3A027319149D8906476E405B301FB9B46838D4F16490B6A4FB7F4C55D86526`.
- `flint-slint` release-бинарь: 32 154 624 байт, GUI subsystem, SHA-256
  `5F266F55…A44CF3`.
- Funded testnet send (копия `dist/data`):
  prepare 1 GRIN, fee `23 500 000` nano, receive-response, finalize; tx
  `dbcd23a3-9aad-4fa6-9be7-e5fb91333476`; получатель принял 1 GRIN.
- Prepared snapshot invalidation: правка адреса убрала снимок и заблокировала
  «Завершить».
- Crash drill funded copy: kill после prepare/response → pending `locked`, только
  «Отменить»; отмена вернула средства. Kill через 80 мс после finalize → `posted`,
  только «Обновить». Ложного `posted`/пустой очереди не было.
- После отмены history-row могла остаться `Отправлено · pending`; retry/cancel
  теперь обновляют activity. В debug-прогоне строка была старой — нужен короткий
  повтор после пересборки debug.
- Инцидент с клонированным funded-кошельком: копии одного seed отправляли
  транзакции, а оригинал видел change как новый incoming и не видел расход
  исходного output — локальный кэш показывал 2904.747 GRIN. Полный
  `scan -h 1` исправил баланс до **1451.862 GRIN**. Клонировать один кошелёк
  и запускать его экземпляры одновременно нельзя; после такого теста нужен
  полный rescan. Временный backup до исправления удалён после проверки
  (оригинальный кошелёк уже пересканирован).
- В Settings добавлена подсказка: после копирования data или нескольких
  экземпляров кошелька выполнить «Сканировать» (полный rescan).
- После полного rescan UI сравнивает spendable до/после и при изменении
  показывает предупреждение «Полный rescan изменил баланс: было … стало …»,
  чтобы исправление кэша не выглядело молчаливой подменой.
- Исправлено: deterministic hook `FLINT_CRASH_BEFORE_FINALIZE` оставлял
  `finalizing`; раньше retry публиковал stored S1 и ставил ложный `posted`.
  Теперь `reconcile_finalizing` не публикует без доказательства подписи:
  запись переходит в `failed_recoverable`; UI оставляет её read-only
  («Обновить»), без auto cancel/publish. Последующий живой F-01 crash/recovery
  подтвердил публикацию исходной подписанной транзакции без повторной подписи.
- `FLINT_CRASH_AFTER_FINALIZE` (S3 уже подписана) — корректно:
  `finalized_not_posted`, только Retry, ровно один S3.
- P1 Settings E2E: битый `settings.json` + валидный `.bak` → приложение стартует,
  показывает warning, битый файл сохранён как `.corrupt-*`, `settings.json`
  восстановлен. Сбой записи `.tmp` не заменяет старый файл.
- P1 backend-gate: прямой `cancel_pending` отказывает для `finalizing`,
  `finalized_not_posted`, `posted`, `failed_recoverable`; запись остаётся.
- P1 messenger: реальный локальный `flint-relay`, 40 битых конвертов + одно
  хорошее; рестарт с durable skip возвращает только хорошее, signed ack
  очищает очередь.
- P1 accessibility: 900×760 настройки без обрезки, 125%/150% scale,
  перенос окна на второй монитор. NVDA/IME и полный Tab-аудит не проверялись.
- P2 package audit: в zip нет `data/`, `.tmp`, `settings.json`, `wallet.seed`;
  release-бинарь в пакете совпадает по SHA-256 с `C:\Flint`;
  `C:\Flint\data` сохранён.
- P2 VC runtime: `Flint.exe`, `flint-selftest.exe` и `grin-wallet.exe`
  импортируют `VCRUNTIME140.dll`; пакет теперь содержит `vcruntime140.dll`
  в корне и в `resources\wallet\`, notice добавлен. Packaged selftest
  проходит. Имитация отсутствующего имени импорта даёт `0xC0000135`;
  реальная чистая Windows без UCRT/VC++ на этой машине недоступна.
- P1 quick block: M2-история уже шифрует `.msg`; добавлены `expires_at` и
  удаление просроченных записей при `record/list/conversations` (тест).
  Добавлены [RESTORE.md](../operations/RESTORE.md), [INCIDENT_RESPONSE.md](../security/INCIDENT_RESPONSE.md)
  (черновик, контакт TBD), [TRUST_BOUNDARY.md](../security/TRUST_BOUNDARY.md) и
  [CALLBACK_AUDIT.md](../security/CALLBACK_AUDIT.md): 43 callback'а проверены, прямой
  `rpc()` из UI убран, confirm-строки паролей переведены в `Zeroizing`.
  Внешний pentest и background node/уведомления остаются открытыми.
- P1 sync/disk/traffic: embedded-скан получил pause/resume между батчами
  (RPC — только cancel); UI показывает «Пауза/Продолжить». Добавлены warning
  при свободном месте < 2 ГиБ и предупреждение о трафике при Headers/Blocks.
- Повторный AI-audit 2026-10-27 выявил текстовую ветку F-01 и добавил R-01; их
  кодовая классификация исправлена и покрыта regression на
  `20261027-f01-r01-candidate`. После этого живой F-01 crash/recovery выполнен, а PT-05
  закрыт живым RPC restore/full-scan. Физический F-04 и остальные live-оговорки сохраняются.
  Датированный снимок аудита: [AUDIT_RECHECK_2026-10-27.md](../security/AUDIT_RECHECK_2026-10-27.md).
- Sentinel-аудит проверен отдельно: SENT-01 (подавление ACL) подтверждён и его
  область оказалась шире отчёта; SENT-02 — `icacls /grant:r` сохранял чужие явные ACE.
  Оба исправлены по коду и покрыты regression на `20261027-sent01-candidate-v2`;
  кодовая часть F-05 также завершена. Второй аккаунт и FAT/exFAT остаются открытыми.
  F-04/F-10 — подтверждённые повторы. Доказательства и границы —
  [SENTINEL_REVIEW_2026-10-27.md](../historical/SENTINEL_REVIEW_2026-10-27.md).
  Это AI/code review и ACL regression, не человеческий pentest.

## Что работает

- Кошелёк: create/restore, open/lock, адрес/ротация, баланс, история, pending,
  activity, seed-envelope, смена пароля, удаление.
- RPC-бэкенд `grin-wallet`: процесс, open/lock, summary/address/activity/pending,
  деньги, seed, switch/settings, shutdown, скан и отмена.
- Managed node: start/stop, фазы, высота, пиры, синхронизация; restart после
  stop (исправлен `ERROR_INVALID_HANDLE`).
- Tor: start/stop/status/bridges/log, onion, bootstrap, `obfs4/snowflake/webtunnel`.
- Messenger: status, send/inbox, history, conversations, contacts, blocked,
  recovery, ключ истории.
- Slint: RPC/Tor/messenger screens, QR, clipboard, настройки, свопы Janus (демо).
- UI: live-статус ноды/скана раз в 2 с, прогресс скана, QR пересобирается только
  при смене адреса, Theme-токены, active/hover NavButton, resizable 900×760+
  с прокруткой контента и clamped-модалками.
- Pending fail-closed: loading/empty/error/unreadable states; unreadable file
  names + diagnostics path; красный блок умеет «Повторить загрузку», команды
  разрешены только для id из успешно загруженного читаемого списка.
- Pending actions: `locked`/`prepared` — только «Отменить»;
  `finalizing`/`finalized_not_posted` — только «Повторить»; `posted` — только
  обновление статуса. То же проверяется command-gate, а не только видимостью кнопок.
- Password policy: общая проверка до history-rewrap; тесты embedded, RPC envelope,
  RPC legacy.
- RPC retry/republish: используется исходная подписанная `.grintx`, а не компактный
  `get_stored_tx`; invalid stage и нулевая подпись отклоняются без публикации.
- Messenger: durable quarantine + signed ack битых конвертов; overfetch не даёт
  мусору скрыть хорошее сообщение.
- Journals: `save_record`/`save_receive` без пути — ошибка, не no-op.
- Settings: atomic write + `settings.json.bak`; corrupt file сохраняется как
  `.corrupt-*`, загрузка показывает warning.
- Send: после prepare показывается неизменяемый снимок recipient/amount/fee;
  редактирование адреса/суммы аннулирует prepared state.
- Lock screen: Enter, Ctrl+V (латиница/русская раскладка), busy-состояние,
  очистка старой ошибки.
- «Получить GRIN»: ScrollView, QR 120×120 с полным quiet zone, кнопки снизу,
  строка синхронизации ноды.

## Доступность/DPI

Проверено UIA на DPI **96 (100%)**:

- названы поля/кнопки, Tab-порядок на lock screen, `IsOffscreen=false` для
  ключевых контролов, offscreen-контролов 0 в messenger;
- Unicode-контакт `Тест` отрисован;
- packaged release: Ctrl+V/Enter и recovery пароля проверены.

Не проверено:

- NVDA/Narrator и реальный IME, полный Tab-аудит всех модалок;
- фактическая чистая Windows без UCRT/VC++ (есть имитация и bundled runtime);
- real non-loopback direct transport и проверка per-IP лимита вне loopback;
- подпись release-сертификатом.

Отдельно, вне текущего счёта: RustSec-обновления зависимостей
([RUSTSEC_UPGRADES.md](../security/RUSTSEC_UPGRADES.md)) — age, clap/yaml-rust и
zip закрыты; остались crypto-bigint (не собирается) и rsa (upstream-блокер).

## Команды

```powershell
cargo test --workspace
pwsh -File tools/ci.ps1
pwsh -File tools/package-slint.ps1
pwsh -File tools/run-testnet-node.ps1 start|status|stop
```

## Документы

- [README.md](../README.md) — единый индекс и правила ведения.
- [PLAN.md](../PLAN.md) — текущий рабочий план.
- [WORK_LOG.md](../WORK_LOG.md) — текущий журнал изменений и проверок.
- [ROADMAP.md](ROADMAP.md) — официальные фазы и гейты.
- [BUILD.md](../../BUILD.md) — сборка.
- [SLINT.md](../ui/SLINT.md) — Slint UI, GPL, MSRV.
- [KRead.md](../KRead.md) — трекер локального плана и доказательств.
- [PSWork.md](../PSWork.md) — внешние приёмки PS-01…PS-17.
- [BACKENDS.md](../architecture/BACKENDS.md) — embedded/RPC.
- [MAINNET_READINESS.md](MAINNET_READINESS.md) — гейт mainnet.
- [TAURI_REMOVAL.md](../historical/TAURI_REMOVAL.md) — backup и журнал удаления Tauri.
- [STATUS_HISTORY.md](../historical/STATUS_HISTORY.md) — исторический журнал 0.1.x.
