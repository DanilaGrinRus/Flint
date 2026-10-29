Команда: cargo test --manifest-path C:\DHW\mobile\GrinFlint\ffi\grin-wallet-ffi\Cargo.toml --test restore_cycle -- --nocapture
Окружение: Windows, локальная floonet-нода http://127.0.0.1:13413, высота ~3314344
Commit: 1b9a62f (тест), Swift-изменения на момент прогона не влияли
Результат: PASS (1 passed, 435.68 s); restore 24 слов в чистом каталоге, адрес совпал, полный скан -> 48.735 tGRIN,
перевод 0.001 tGRIN подтверждён за 9 раундов, отмена после подтверждения отклонена.
Секреты в лог не попадали (seed/пароль/API-секрет передавались только через env).
