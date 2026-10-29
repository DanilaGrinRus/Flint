# Спайк: отмена embedded-скана (0.1.69)

> **HISTORICAL:** документ относится к периоду до удаления Tauri (2026-10-26). Ссылки на `crates/app`/`ui/` и старые `--selftest`/UI-проверки — исторические.


**Статус:** спайк проведён 2026-10-25; вариант A реализован в 0.1.69
(`EmbeddedScanJob`, кооперативная отмена между батчами). Ниже — исходный разбор
и решение; фактическое состояние — в
[STATUS.md](../status/STATUS.md#отмена-скана--0169).

## 1. Задача и прежняя граница

RPC-отмена сделана в 0.1.69: отдельный процесс `grin-wallet` убивается, скан
возвращает ошибку, состояние становится `failed`, backend перезапускается.
Для embedded так нельзя: `Owner::scan` синхронный, внешнего процесса нет, а
аварийное завершение потока во время записи в LMDB недопустимо.

В прошлом срезе предполагалось, что батчевую логику libwallet придётся
копировать или выносить embedded-скан в отдельный процесс. Спайк проверил
вендоренную копию `grin_wallet_libwallet 5.5.1` и это предположение не
подтвердилось.

## 2. Что реально есть в вендоренном 5.5.1

- `vendor/grin_wallet_libwallet/src/lib.rs:74` — `pub use
  internal::scan::scan;`. Модуль `internal` приватный, но сама функция
  `scan` публично реэкспортирована.
- `vendor/grin_wallet_api/src/owner.rs:76` — поле `Owner::wallet_inst`
  **публичное**: `Arc<Mutex<Box<dyn WalletInst<...>>>>`. Его можно клонировать и
  работать с тем же экземпляром кошелька, который держит `Owner`.
- `vendor/grin_wallet_libwallet/src/internal/scan.rs:466` — публичная
  `scan(...)` принимает:
  `batch_start_height`, `batch_end_height`, полный `start_height`/`end_height`,
  `Option<(u64, u64)>` для PMMR-диапазона и канал `StatusMessage`. Комментарий
  прямо говорит, что функция нужна тем, кто «drive a scan in batches and save
  progress between those batches».
- `vendor/grin_wallet_libwallet/src/api_impl/owner.rs:1016` — высокоуровневый
  `owner::scan`: `update_outputs`, tip, затем цикл
  `(start..tip + 1).step_by(10001)`, после каждого батча
  `save_last_scanned_block`. Это ~25 строк оркестрации; сам скан, rewind и
  запись выходов остаются внутри libwallet.
- `vendor/grin_wallet_libwallet/src/internal/updater.rs:406` и реэкспорт
  `lib.rs:76` — публичная `refresh_outputs`. Приватная `update_outputs` в
  `api_impl/owner.rs:1337` — тонкая обёртка над ней; её можно заменить вызовом
  `refresh_outputs` через `lc_provider().wallet_inst()`.
- `WalletBackend::batch` (`backend.rs:437`) и
  `WalletBatch::save_last_scanned_block` (`backend.rs:626`) доступны для
  сохранения прогресса между батчами.
- В embedded-режиме Flint **не запускает updater-поток** `Owner`
  (`start_updater` нигде не вызывается), поэтому отдельного конкурентного
  сканера, с которым нужно договариваться, нет.

Публичность этих элементов проверена компиляцией временного теста
`cargo check -p flint-core --test ...`: вызовы `scan`, `refresh_outputs`,
`Owner::wallet_inst`, `batch` и `save_last_scanned_block` собираются с типами
embedded-кошелька. Тест после проверки удалён; это был только compile-check,
не проверка поведения.

## 3. Вариант A — кооперативная отмена по батчам (рекомендуется)

Не убивать поток и не патчить vendor. Вместо `Owner::scan` embedded-скан
водится циклом по публичной `scan` и проверяет флаг отмены **между батчами**.

Эскиз:

```rust
// &mut WalletService, уже unlocked
let owner = self.owner()?;                 // Owner<Lc, HTTPNodeClient, ExtKeychain>
let wallet = owner.wallet_inst.clone();
let tip = owner.node_height(None)?;        // height + header_hash
if !tip.updated_from_node { return Err(...); }

// как в owner::scan: обновить известные выходы перед полным сканом
{
    let mut guard = wallet.lock();
    let w = guard.lc_provider()?.wallet_inst()?;
    let parent = w.parent_key_id();
    refresh_outputs(w, None, &parent, true)?;
}

let start = start_height.unwrap_or(1);
let mut total_pmmr_range = None;
for h in (start..tip.height + 1).step_by(10_001) {
    if cancel.load(Ordering::Relaxed) {
        return Err(ScanCancelled);         // cancel_scan(...), pending_scan не трогать
    }
    let batch_end = std::cmp::min(tip.height, h + 10_000);
    let (mut info, range) = grin_wallet_libwallet::scan(
        wallet.clone(), None, false,
        h, batch_end, start, tip.height,
        total_pmmr_range, &None,
    )?;
    info.hash = if batch_end == tip.height { tip.header_hash.clone() } else { String::new() };
    total_pmmr_range = Some(range);

    let mut guard = wallet.lock();
    let w = guard.lc_provider()?.wallet_inst()?;
    let mut batch = w.batch(None)?;
    batch.save_last_scanned_block(info)?;
    batch.commit()?;
}
```

Что это даёт:

- **Безопасность LMDB:** отмена происходит только на границе батча; никакого
  `abort`/`process::exit`/убийства потока.
- **Сохранение прогресса:** каждый батч пишет `last_scanned_block`, как
  upstream; следующий скан продолжит с сохранённой точки.
- **Без патча vendor:** используется публичный API и публичное поле `Owner`.
  Копируется только оркестрирующий цикл (~25 строк), а не логика rewind,
  восстановления выходов и записи в БД.
- **Совместимость по смыслу:** отмена — мягкая, «после текущего батча», а не
  мгновенная. Для embedded это честный компромисс; UI должен писать
  «Отменяю… (после текущего батча)».
- **Ошибки:** если батч упал до отмены — обычный `fail_scan`; если отмена
  пришла после последнего батча — `complete_scan` побеждает, отмена не
  перезаписывает `Completed`.

### Что понадобилось для реализации (выполнено)

1. `WalletService`: токен отмены (`Arc<AtomicBool>` или небольшой
   `ScanCancel`), метод `scan_wallet_cooperative`; `ScanState` и
   `pending_scan` ведутся как сейчас.
2. `AppState`: отдельный флаг для embedded (не путать с RPC `ScanControl`),
   `scan_busy` выставляется и для embedded-скана.
3. IPC `wallet_scan_cancel`: для RPC — текущий kill; для embedded — только
   `store(true)` и ответ `{"cancelling": true}`. Ветка выбирается по
   `has_rpc`.
4. `WalletStatus.scan_cancellable`: сейчас `has_rpc && scan_busy`; станет
   `scan_busy` для обоих бэкендов (с разным текстом подтверждения).
5. UI: для embedded кнопка появляется, подтверждение объясняет «остановится
   после текущего батча», без перезапуска процесса.
6. Тесты:
   - до первого батча → `failed`, `pending_scan` сохранён;
   - отмена после завершения → `Completed` не перезаписывается;
   - повторная отмена → «скан не идёт»;
   - живой ignored-тест с настоящей testnet-нодой: скан → отмена → `failed` →
     повторный скан стартует и завершается.
7. Документация: STATUS/TRIAGE/ROADMAP, честная граница «мягкая отмена».

### Риски и открытые вопросы

- Публичная `scan` помечена как low-level: upstream рекомендует вызывать
  owner-методы. При обновлении vendor сигнатура может измениться — тест и
  пин `5.5.1` обязательны.
- Один батч — до 10 001 блока. На медленном диске/CPU отмена может ждать
  десятки секунд; это нужно измерить на живой ноде, а не обещать мгновенность.
- `refresh_outputs` возвращает ошибку при недоступной ноде, тогда как
  высокоуровневый `update_outputs` её проглатывает. Нужно решить: падать сразу
  или дать батчу вернуть свою ошибку. Рекомендация — не скрывать ошибку узла,
  но текст должен быть понятным.
- Прогресс в UI по-прежнему не процент: можно показывать только «батч N из M»
  или ничего. В срез это не обязательно.
- Нужен отдельный признак «отмена запрошена, ждём границу батча», чтобы UI не
  выглядел зависшим.

## 4. Альтернативы

| Вариант | Плюсы | Минусы | Решение |
|---|---|---|---|
| A. Кооперативные батчи через публичный API | Без vendor-патча, без процесса, безопасно для LMDB, прогресс сохраняется, ~один срез | Мягкая отмена, задержка до конца батча, дублируется ~25 строк оркестрации | **Рекомендуется** |
| B. Вынести embedded-скан в отдельный процесс | Жёсткая отмена, как RPC | Нужны передача seed/пароля, блокировка БД, IPC, запуск/перезапуск, упаковка; недели и высокий риск | Отложить; не в этом срезе |
| C. Патч vendor: добавить флаг в `owner::scan` | Минимум кода в Flint | Форк upstream, конфликты при обновлении, офлайн-vendor дрейфует; против правил проекта | Отклонить |
| D. Оставить явный отказ | Ноль риска | Пользователь не может отменить восстановление; UX-долг | Текущее состояние |
| E. `abort` потока / kill процесса Flint | Мгновенно | Риск повреждения LMDB и потери ключей; недопустимо | Отклонить |

## 5. Рекомендация

Вариант **A** выбран и реализован отдельным срезом 0.1.69. Это не «нажать
кнопку»: понадобились токен отмены, новый цикл, ветка IPC, статус, UI-тексты и
живой тест. Но в отличие от B он не трогает формат кошелька, seed, блокировки и
упаковку, а в отличие от C не создаёт форк upstream.

До реализации текущее поведение было честным отказом; после — embedded-скан
останавливается на границе батча, а прерванный прогресс сохраняется.

## 6. Что проверено

- Прочитаны конкретные места вендоренного 5.5.1 (ссылки выше), не весь vendor.
- Проверена публичность `scan`, `refresh_outputs`, `Owner::wallet_inst`,
  `WalletBackend::batch` и `save_last_scanned_block` компиляцией временного
  теста; тест удалён.
- В реализации: `EmbeddedScanJob` водит батчи, `wallet_scan` отпускает мьютекс
  `WalletService` на время скана, статус и отмена доступны во время работы.
  Unit-тесты: `finish_scan_applies_cooperative_outcomes`,
  `a_late_batch_cancel_does_not_overwrite_completed`.
- Постоянный ignored-тест `live_embedded_scan_cancel::embedded_scan_cancels_between_batches`
  (FLINT_LIVE_EMBEDDED_SCAN=1) прошёл на testnet за 2,43 с: отмена на границе
  батча, `failed`/`pending_scan`, повторный скан завершился `completed`.
- Живой прогон в собранном приложении (testnet, высота 3 308 493): embedded-
  скан, кнопка «Отменить скан», подтверждение с текстом про батч, отмена из UI
  за 132–383 мс, `failed` с причиной, кошелёк остался разблокированным,
  повторный скан от tip-2000 завершился `completed`.
- Перед явным сканом `wallet_summary` не запускает скрытый full scan: новый и
  восстановленный кошелёк помечаются `InitNoScanning`, `refresh` после создания
  занял 206 мс; после успешного `wallet_scan` ставится `InitComplete`.
- Не проверено живым прогоном: сохранение `pending_scan` при отмене **после
  restore** в embedded-режиме (покрыто unit-тестом `finish_scan`), и задержка
  отмены на медленном диске/mainnet.
