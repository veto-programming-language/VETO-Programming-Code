#!/usr/bin/env python3
# gen_train.py — генератор сбалансированного train.csv для Инквизитора Veto.
# 510 approve + 510 veto = 1020 строк ровно.
# Запуск: python3 gen_train.py

import csv
import random

# ============================================================
# APPROVE — 510 строк (доказательства)
# ============================================================

APPROVE_BASE = [
    # Русские формальные
    "пусть speed ∈ [0..100], boost ∈ [0..50], тогда speed+boost ∈ [0..150] < 200",
    "пусть a ∈ [0..100], b ∈ [0..50], тогда a+b ∈ [0..150] < 200",
    "интервал [0..150] полностью входит в [0..200], следовательно условие доказано",
    "∀ s∈[0..100], b∈[0..50]: s+b ≤ 150 < 200, ч.т.д.",
    "по теореме о границах: min+min=0, max+max=150, 150<200",
    "speed ∈ [0..100], boost ∈ [0..50] => sum ∈ [0..150] => sum < 200",
    "если 0<=speed<=100 и 0<=boost<=50, то 0<=speed+boost<=150",
    "максимум суммы = 100 + 50 = 150; 150 < 200 следовательно доказано",
    "интервальная арифметика: [0,100]+[0,50]=[0,150] ⊂ (-∞,200)",
    "для любого x в [0..100] и y в [0..50] верно x+y <= 150 < 200",
    "пусть a∈[0,100], b∈[0,50]; тогда a+b≤150; 150<200; доказано",
    "исходя из границ типов speed и boost сумма не превышает 150",
    "минимум 0, максимум 150, оба меньше 200, условие выполнено",
    "0 <= speed, speed <= 100, 0 <= boost, boost <= 50, значит сумма <= 150",
    "по аксиоме порядка: a<=100 и b<=50 => a+b<=150",
    "пусть speed=100, boost=50 — худший случай; 100+50=150<200, доказано",
    "worst case: speed=100, boost=50, sum=150, порог 200 не превышен",
    "рассмотрим крайние значения: 0+0=0, 100+50=150; оба < 200",
    "по индукции: база 0+0=0<200, шаг: приращение <= 150, ч.т.д.",
    "максимальное значение суммы 150 меньше 200, переполнения нет",
    "переполнение невозможно, так как 150 < 2^31-1",
    "сложение двух положительных чисел из диапазонов даёт [0..150], влезает в i32",
    "arr[i] с i∈[0..N-1] безопасен, потому что 0<=i<N по контракту",
    "индекс i принимает значения 0..N-1, следовательно i<N всегда",
    "∀ i∈[0..N): i < N, поэтому arr[i] не выходит за границы",
    "длина массива N, индекс i ∈ [0, N-1] ⊂ [0, N), ч.т.д.",
    "если i < N и i >= 0, то чтение arr[i] безопасно по определению",
    "индекс в границах [0..N-1], доступ безопасен",
    "умножение i32<0..10> на i32<0..10> даёт i32<0..100>, переполнения нет",
    "диапазоны [0,10]*[0,10]=[0,100] помещаются в i32, доказано",
    "по правилу интервалов: 10*10=100 <= i32::MAX, следовательно безопасно",
    "переполнения нет, потому что max=max_a*max_b=100 < 2^31-1",
    "сложение [0..100]+[0..50] даёт [0..150], влезает в i32 без переполнения",
    "сумма двух положительных не превосходит 150 < 2^31-1, ч.т.д.",
    "вычитание 100-50=50, в диапазоне i32, переполнения нет",
    "умножение [0..255]*[0..255]=[0..65025], помещается в i32",
    "сложение [0..255]+[0..255]=[0..510], влезает в i32 без переполнения",
    "диапазон [0..511] умноженный на 2 даёт [0..1022], помещается в i32",
    # Английские формальные
    "let a in [0..100], b in [0..50], then a+b in [0..150] < 200",
    "sum interval [0..150] is fully contained in [0..200], hence safe",
    "forall s in [0..100], b in [0..50]: s+b <= 150 < 200, q.e.d.",
    "by interval arithmetic: [0,100]+[0,50]=[0,150] subset of (-inf,200)",
    "worst case: s=100, b=50, sum=150, threshold 200 not exceeded",
    "max sum = 100+50=150, 150<200, condition proven",
    "since 0<=a<=100 and 0<=b<=50, then 0<=a+b<=150",
    "the sum cannot overflow i32 because 150 < 2^31-1",
    "index i is in [0..N-1], hence i<N, array access is safe",
    "forall i in [0..N): i < N, therefore arr[i] is in bounds",
    "product [0..10]*[0..10]=[0..100] fits in i32, no overflow",
    "a+b is at most 150, which is less than 200, proof complete",
    "consider min=0+0=0, max=100+50=150, both less than 200",
    "by induction: base 0<200, step adds at most 150, q.e.d.",
    "if a<=100 and b<=50, then a+b<=150 by the order axiom",
    "array of length N, i in [0,N-1] subset of [0,N), safe",
    "multiplication [0..255]*[0..255]=[0..65025] fits in i32",
    "since i>=0 and i<N, arr[i] is safe by definition",
    "the maximum of a+b is 150, which is strictly less than 200",
    "interval sum [0..150] subset [0..200], condition holds",
]

# Добиваем APPROVE до 510 через перестановки диапазонов
RANGES = [
    (0, 100, 0, 50, 200),
    (0, 255, 0, 255, 512),
    (0, 511, 0, 1, 1024),
    (0, 1000, 0, 500, 2000),
    (0, 10, 0, 10, 100),
    (0, 50, 0, 30, 200),
    (1, 100, 1, 50, 200),
    (0, 127, 0, 127, 255),
    (0, 65535, 0, 1, 131072),
    (0, 1024, 0, 1024, 4096),
    (0, 200, 0, 100, 500),
    (0, 300, 0, 200, 1000),
    (0, 15, 0, 15, 100),
    (0, 20, 0, 20, 200),
    (5, 50, 5, 50, 200),
]
NAMES = [
    ("speed", "boost"),
    ("a", "b"),
    ("x", "y"),
    ("left", "right"),
    ("min_val", "max_val"),
    ("lo", "hi"),
    ("first", "second"),
    ("p", "q"),
    ("i", "j"),
    ("row", "col"),
]
APPROVE_TEMPLATES = [
    "пусть {a} ∈ [{lo1}..{hi1}], {b} ∈ [{lo2}..{hi2}], тогда {a}+{b} ∈ [{slo}..{shi}] < {bound}",
    "интервал суммы [{lo1}..{hi1}]+[{lo2}..{hi2}] = [{slo}..{shi}] ⊂ (-∞,{bound})",
    "∀ {a}∈[{lo1}..{hi1}], {b}∈[{lo2}..{hi2}]: {a}+{b} ≤ {shi} < {bound}, ч.т.д.",
    "по теореме о границах: min={slo}, max={shi}, {shi} < {bound}",
    "если {lo1}<={a}<={hi1} и {lo2}<={b}<={hi2}, то {slo}<={a}+{b}<={shi}",
    "максимум суммы = {hi1} + {hi2} = {shi}; {shi} < {bound} доказано",
    "интервальная арифметика: [{lo1},{hi1}]+[{lo2},{hi2}]=[{slo},{shi}] ⊂ (-∞,{bound})",
    "для любого x в [{lo1}..{hi1}] и y в [{lo2}..{hi2}] верно x+y <= {shi} < {bound}",
    "пусть a∈[{lo1},{hi1}], b∈[{lo2},{hi2}]; тогда a+b≤{shi}; {shi}<{bound}; доказано",
    "пусть {a}={hi1}, {b}={hi2} — худший случай; {hi1}+{hi2}={shi}<{bound}",
    "рассмотрим крайние значения: {lo1}+{lo2}={slo}, {hi1}+{hi2}={shi}; оба < {bound}",
    "let {a} in [{lo1}..{hi1}], {b} in [{lo2}..{hi2}], then {a}+{b} in [{slo}..{shi}] < {bound}",
    "sum interval [{lo1}..{hi1}]+[{lo2}..{hi2}] = [{slo}..{shi}] subset of (-inf,{bound})",
    "forall {a} in [{lo1}..{hi1}], {b} in [{lo2}..{hi2}]: {a}+{b} <= {shi} < {bound}",
    "if {lo1}<={a}<={hi1} and {lo2}<={b}<={hi2}, then {slo}<={a}+{b}<={shi}",
    "max sum = {hi1}+{hi2}={shi}, {shi} < {bound}, condition proven",
    "worst case {a}={hi1}, {b}={hi2}: sum={shi} < {bound}, q.e.d.",
]

# ============================================================
# VETO — 510 строк (отмазки, RU + EN)
# ============================================================

VETO_BASE = [
    # === ENGLISH (Reddit / SO memes) ===
    "works on my machine",
    "it works on my machine, i swear",
    "it compiled, ship it",
    "if it compiles, ship it",
    "it should work now",
    "i don't know why it works",
    "it worked yesterday",
    "it worked five minutes ago",
    "trust me bro",
    "just trust me bro",
    "trust me, it won't be null",
    "i'm sure it's fine",
    "it's probably fine",
    "no need to test, it's obvious",
    "i tested it on my machine",
    "i ran it once and it worked",
    "no errors on my end",
    "works for me",
    "can't reproduce, must be your environment",
    "must be a compiler bug",
    "it's a libc bug",
    "it's a kernel bug",
    "it's a hardware issue",
    "works on my laptop",
    "works in docker, so it's fine",
    "the tests pass, so it's correct",
    "tests are green, we're good",
    "ci passed, so it works",
    "it's just a simple change, nothing can break",
    "what could possibly go wrong",
    "it's a trivial change",
    "i've done this a thousand times",
    "this has always worked",
    "nobody else complained",
    "it's not a bug, it's a feature",
    "it's not a bug, it's undefined behavior",
    "it's not my code, it's the library",
    "blame the compiler",
    "blame the network",
    "blame the database",
    "blame the user",
    "it's an edge case, nobody will hit it",
    "edge cases don't matter in production",
    "nobody will ever pass null there",
    "nobody will ever input that",
    "the input is always valid",
    "it's a small array, bounds don't matter",
    "the numbers are small, nothing overflows",
    "nobody uses more than 4gb anyway",
    "i'll fix it later",
    "we'll fix it in the next release",
    "it's on the backlog",
    "it's a known issue",
    "it works in dev, prod will be fine",
    "it's friday, ship it",
    "the boss said it's fine",
    "the pm approved it",
    "we don't have time for tests",
    "i don't make mistakes",
    "i wrote it, so i know it's correct",
    "i'm the author, trust me",
    "don't question my code",
    "the math is obvious, no need to prove",
    "it's intuitively obvious",
    "everyone knows this is safe",
    "everyone does it this way",
    "i saw it on stack overflow",
    "the ai wrote it, so it's correct",
    "copilot suggested it",
    "chatgpt said it's fine",
    "it compiles without warnings, so it's safe",
    "warnings are just suggestions",
    "clippy is too strict, my code is fine",
    "the analyzer is confused",
    "runtime checks are for cowards",
    "bounds checks are slow, disable them",
    "no one needs safety",
    "safety is for other languages",
    "if it crashes, it crashes fast",
    "fast code is correct code",
    "performance > correctness",
    "o3 will fix it",
    "gcc knows better than me",
    "the optimizer will handle it",
    "undefined behavior is just a suggestion",
    "ub doesn't happen in practice",
    "it's implementation-defined",
    "nobody uses 32-bit anymore",
    "nobody uses that compiler",
    "nobody uses that os",
    "it's out of scope",
    "not my department",
    "not my problem",
    "wontfix, works as intended",
    "by design, not a bug",
    "if there's no test, it's not a bug",
    "untested code is bug-free code",
    "no test, no problem",
    "tests slow me down",
    "i test in production",
    "users are the best testers",
    "it's a prototype, quality doesn't matter",
    "it's just internal, nobody will see it",
    "it's behind a firewall, it's safe",
    "it's a hack, it's temporary",
    "temporary code is fine",
    "good enough for now",
    "ship it and pray",
    "deploy on friday, what could go wrong",
    "we can always rollback",
    "hotfix in prod is fine",
    "release first, fix later",
    "move fast and break things",
    "fail fast, learn faster",
    "speed is safety",
    "real programmers don't need safety",
    "real programmers don't test",
    "real programmers ship on friday",
    "real programmers don't need proofs",
    "proofs are for mathematicians, not programmers",
    "who needs formal verification",
    "formal methods are academic nonsense",
    "i don't have time for proofs",
    "just run it and see",
    "run it and pray",
    "if it runs, it's correct",
    "no segfault, no bug",
    "segfault is just a feature",
    "integer overflow is a myth",
    "floating point is exact",
    "floats are precise enough",
    "precision doesn't matter for this",
    "rounding errors are negligible",
    "two wrongs make a right",
    "if it's wrong, it's wrong consistently",
    "known bug is not a bug",
    "the docs say it works, so it works",
    "i didn't read the docs, but i'm sure it's fine",
    "reading docs is cheating",
    "breaking prod is the best teacher",
    "outages build character",
    "at least it's not java",
    "at least it's not javascript",
    "at least it's not php",
    "at least it's not python",
    "at least it's not rust",
    "rust would reject this, but veto is better",
    "if veto crashes, it crashes at native speed",
    "correctness is secondary",
    "speed is the only metric",

    # === РУССКИЕ ===
    "у меня работает",
    "у меня всё работает",
    "у меня локально всё работает",
    "у меня на машине работает",
    "у меня собирается",
    "у меня компилируется",
    "у меня запускается",
    "у меня не падает",
    "у меня всё ок",
    "у меня всё нормально",
    "всё собирается, значит всё ок",
    "компилируется — значит правильно",
    "собирается без ошибок",
    "билд зелёный, можно деплоить",
    "тесты проходят, значит всё работает",
    "тесты зелёные, всё ок",
    "сиай прошёл, всё хорошо",
    "на проде проверим",
    "на проде разберёмся",
    "на проде не упадёт",
    "на проде пользователи не заметят",
    "на проде другие условия",
    "в проде всё по-другому",
    "в проде не так",
    "в проде норм",
    "в тестах работает",
    "в докере работает",
    "в виртуалке работает",
    "в песочнице работает",
    "на моей машине работает",
    "на моём ноуте работает",
    "на моём сервере работает",
    "на локалке всё ок",
    "локально воспроизвести не могу",
    "не воспроизводится",
    "не могу воспроизвести",
    "у меня не воспроизводится",
    "значит это не баг",
    "значит проблема у тебя",
    "значит у тебя что-то не так",
    "значит у тебя сломано",
    "проблема не в моём коде",
    "это не мой код",
    "это библиотека виновата",
    "это фреймворк виноват",
    "это компилятор виноват",
    "это ос виновата",
    "это сеть виновата",
    "это база виновата",
    "это железо виновато",
    "это пользователи виноваты",
    "пользователь что-то нажал не то",
    "пользователь сам виноват",
    "пользователи всегда что-то ломают",
    "это не баг, это фича",
    "это не баг, это недокументированная возможность",
    "это не баг, это андефайнед бихейвиор",
    "это не баг, это рейс кондишн",
    "это не баг, это тайминги",
    "это не баг, это поведение по умолчанию",
    "это работает как задумано",
    "так и должно быть",
    "это в спеке",
    "в спеке не сказано что должно работать",
    "в контракте не запрещено",
    "значит это допустимо",
    "значит это нормально",
    "значит это ок",
    "хз, но работает",
    "хз почему, но работает",
    "хз как, но работает",
    "не знаю почему, но работает",
    "работает — не трогай",
    "работает и ладно",
    "работает, значит правильно",
    "главное что работает",
    "зачем трогать, если работает",
    "и так сойдёт",
    "и так работает",
    "и так нормально",
    "и так пойдёт",
    "пойдёт и так",
    "сойдёт для продакшена",
    "для продакшена сойдёт",
    "для прототипа сойдёт",
    "для мвп сойдёт",
    "потом пофиксим",
    "потом поправим",
    "потом доделаем",
    "потом разберёмся",
    "потом перепишем",
    "в следующем релизе пофиксим",
    "в следующем спринте поправим",
    "в бэклоге лежит",
    "в бэклоге есть",
    "это известная проблема",
    "это известный баг, мы работаем над ним",
    "это давно известно",
    "это не приоритет",
    "не сейчас",
    "не сегодня",
    "не в этот раз",
    "не в этот спринт",
    "не моя зона ответственности",
    "не моя задача",
    "не мой тикет",
    "не моя проблема",
    "я тут ни при чём",
    "я не виноват",
    "я не трогал",
    "я не менял",
    "я не при делах",
    "это не я",
    "это не моя вина",
    "виноват тот кто писал до меня",
    "предыдущий разработчик виноват",
    "легаси виновато",
    "легаси код — это не моя проблема",
    "легаси не трогаем",
    "легаси работает — не трогай",
    "не трогай, работает же",
    "работает — не чини",
    "не ломай то что работает",
    "не чини то что не сломано",
    "если не сломано — не чини",
    "зачем чинить, если работает",
    "мне некогда",
    "нет времени",
    "нет времени на тесты",
    "тесты — это долго",
    "тесты — для джунов",
    "тесты пишут те кто ошибается",
    "я не ошибаюсь",
    "я написал — значит правильно",
    "я автор — мне виднее",
    "я знаю этот код лучше всех",
    "я тут дольше тебя",
    "не questioning мой код",
    "не задавай лишних вопросов",
    "ты не понимаешь",
    "тебе не понять",
    "это сложно объяснить, просто поверь",
    "математика очевидная, доказывать не надо",
    "интуитивно понятно",
    "всем известно что это безопасно",
    "это общеизвестно",
    "это стандартная практика",
    "все так делают",
    "я видел на стек оверфлоу",
    "на хабре писали",
    "в статье было",
    "в туториале так было",
    "ии написал — значит правильно",
    "копилот подсказал",
    "чатгпт сказал что ок",
    "нейросеть плохого не посоветует",
    "сгенерированный код — правильный по определению",
    "типы сходятся — значит код правильный",
    "компилируется без варнингов — значит безопасно",
    "варнинги — это просто рекомендации",
    "я отключил варнинги — проблема решена",
    "я игнорирую варнинги, всё ок",
    "линтер ошибается, а не мой код",
    "клиппи слишком строгий, мой код норм",
    "компилятор ошибается, мой код правильный",
    "это фолс позитив",
    "анализатор запутался",
    "статический анализ — это оверинжиниринг",
    "рантайм проверки — для трусов",
    "банд чек медленный, отключим",
    "зачем проверки, и так быстро",
    "безопасность — для других языков",
    "в вето нет проверок, значит быстро",
    "если упадёт — упадёт быстро",
    "быстрый код — правильный код",
    "производительность важнее корректности",
    "о3 всё пофиксит",
    "гцц умнее меня",
    "оптимизатор разберётся",
    "уб — это просто рекомендация",
    "уб в реальности не случается",
    "это технически уб, но работает",
    "это имплементационно-зависимо, но консистентно",
    "это платформозависимо, но нам норм",
    "это архитектурозависимо, но x86 решает",
    "на интеле работает, кому важен arm",
    "литл эндиан — единственный эндиан",
    "64-бит — единственная битность",
    "никто не использует 32-бит",
    "никто не использует этот компилятор",
    "никто не использует эту ос",
    "никто не использует эту версию",
    "эта конфигурация не поддерживается",
    "это вне скоупа",
    "не моё дело",
    "воркс он май машин, закрываю как wontfix",
    "не буду фиксить, работает как задумано",
    "бай дизайн, не баг",
    "документированное поведение",
    "в спеке не сказано что не должно падать",
    "спека неоднозначна, значит ок",
    "контракт не запрещает",
    "нет теста — значит не требуется",
    "если нет теста — это не баг",
    "нетестированный код — безбаговый",
    "нет теста — нет проблемы",
    "я не писал тесты, потому что очевидно",
    "тесты меня тормозят",
    "я тестирую в проде",
    "прод — лучшая тестовая среда",
    "пользователи — лучшие тестеры",
    "пусть пользователи найдут баги",
    "бета-тестеры поймают",
    "это альфа, баги ожидаемы",
    "это прототип, качество не важно",
    "это пруф оф консепт, безопасность не нужна",
    "это внутренний код, никто не увидит",
    "это за файрволом, безопасно",
    "это только локалхост, безопасность не нужна",
    "это оффлайн, ничего не случится",
    "это демо, работать не обязательно",
    "это хак, это временно",
    "временный код — это норм",
    "нет ничего более постоянного чем временное",
    "это воркэраунд, не фикс",
    "настоящий фикс слишком сложен",
    "правильное решение слишком долго",
    "для сейчас сойдёт",
    "шип ит энд прей",
    "деплой в пятницу — что может пойти не так",
    "роллбэк — для трусов",
    "мы всегда можем откатиться",
    "роллбэк быстрее тестирования",
    "хотфикс в проде — норм",
    "запатчим после релиза",
    "сначала релиз, потом фикс",
    "двигаемся быстро, ломаем вещи",
    "падаем быстро, учимся быстрее",
    "упал рано — упал часто",
    "если не падает — недостаточно быстро",
    "скорость — это безопасность",
    "быстро потому что без проверок",
    "проверки — это оверхед",
    "проверки безопасности — для джунов",
    "настоящие программисты не нуждаются в безопасности",
    "настоящие программисты пишут на ассемблере",
    "настоящие программисты не тестируют",
    "настоящие программисты шипят в пятницу",
    "настоящие программисты не читают доки",
    "настоящие программисты не доказывают",
    "доказательства — для математиков, не для программистов",
    "это не экзамен по математике, это код",
    "это не теорема, это программа",
    "кому нужна формальная верификация",
    "формальные методы — академическая чушь",
    "модел чекинг — оверкилл",
    "теорем прувинг — для пхд",
    "у меня нет времени на доказательства",
    "просто запусти и посмотри",
    "запусти и помолись",
    "если запускается — значит правильно",
    "если не падает сразу — значит ок",
    "нет сегфолта — нет бага",
    "сегфолт — это фича",
    "кордамп — это любовное письмо",
    "порча памяти — это приключение",
    "переполнение буфера — это скилл ишью",
    "use after free — это лайфстайл",
    "дабл фри — двойное удовольствие",
    "нулл дереф — это обряд посвящения",
    "интеджер оверфлоу — это миф",
    "флоатинг поинт точен",
    "0.1 + 0.2 == 0.3",
    "флоаты достаточно точные",
    "точность тут не важна",
    "ошибки округления незначительны",
    "это маленькая ошибка, не накопится",
    "ошибки компенсируются",
    "два минуса дают плюс",
    "если неправильно — то стабильно неправильно",
    "консистентность важнее корректности",
    "предсказуемые баги лучше непредсказуемых фиксов",
    "я понимаю баг — значит это ок",
    "известный баг — не баг",
    "документированный баг — фича",
    "если в доках — значит правильно",
    "доки говорят что работает — значит работает",
    "доки врут, мой код прав",
    "я не читал доки, но уверен",
    "чтение доков — для читеров",
    "я учусь ломая",
    "ломать прод — лучший учитель",
    "аутажи строят характер",
    "даунтайм — это перерыв",
    "пользователям тоже нужен перерыв от моего кода",
    "мой код настолько быстрый, что краш не видно",
    "краш мгновенный — значит быстрый",
    "быстрые краши лучше медленных багов",
    "хотя бы не джава",
    "хотя бы не джаваскрипт",
    "хотя бы не пхп",
    "хотя бы не питон",
    "хотя бы не раст",
    "раст бы это отверг, но вето лучше",
    "вето не нужна безопасность, он быстрый",
    "вето быстрее раста потому что без проверок",
    "если вето падает, то на нативной скорости",
    "нативные краши эффективнее",
    "эффективность — это всё",
    "корректность вторична",
    "скорость — единственная метрика",
    "если быстро — значит хорошо",
    "медленно и правильно хуже быстро и неправильно",
    "я лучше быстро и неправильно",
    "быстрый неправильный код всё равно быстрый",
]

# Добиваем VETO до 510 через вариации
VETO_SUFFIXES = [
    "",
    ", честно",
    ", поверь",
    ", я проверял",
    ", всё точно ок",
    ", отвечаю",
    ", зуб даю",
    ", без шуток",
    ", я уверен",
    ", интуиция подсказывает",
    ", ну ты понял",
    ", да ладно тебе",
    ", не парься",
    ", это же очевидно",
    ", что тут думать",
    ", я 10 лет так пишу",
    ", это стандартный паттерн",
    ", так все делают",
    ", на проде проверим",
    ", потом разберёмся",
    ", сейчас не до этого",
    ", давай уже собирать",
]

def build_veto():
    seen = set()
    out = []
    for line in VETO_BASE:
        if line not in seen:
            seen.add(line)
            out.append(line)
    if len(out) >= 510:
        return out[:510]
    # Добиваем суффиксами
    for base in VETO_BASE:
        for suf in VETO_SUFFIXES:
            if not suf:
                continue
            line = base + suf
            if line not in seen:
                seen.add(line)
                out.append(line)
            if len(out) >= 510:
                return out
    return out

# ============================================================
# MAIN
# ============================================================

# ============================================================
# ТРУДНЫЕ ПРИМЕРЫ + СБОРКА
# ============================================================

HARD_VETO = [
    "я проверял на {n} входах, сумма всегда меньше {bound}, поверь",
    "тесты прошли {n} раз подряд, значит {hi1}+{hi2} < {bound}, честно",
    "пусть {a} ∈ [{lo1}..{hi1}], тогда всё ок, зуб даю",
    "{a} ∈ [{lo1}..{hi1}], {b} ∈ [{lo2}..{hi2}] => {a}+{b} < {wrong}",
    "у меня {n} тестов зелёные, переполнения быть не может",
]

HARD_APPROVE = [
    "максимум первого {hi1}, максимум второго {hi2}, их сумма {shi}, это меньше порога {bound}, значит безопасно",
    "обе величины неотрицательны и не больше {hi1} и {hi2}, поэтому сумма не больше {shi}, а {shi} < {bound}",
    "{a} не больше {hi1}, {b} не больше {hi2}, поэтому {a}+{b} не больше {shi}, а {shi} меньше {bound}, переполнения нет",
    "наименьшая сумма {slo}, наибольшая {shi}, и {shi} меньше {bound}, значит {a}+{b} всегда в пределах",
    "для {a} верхняя граница {hi1}, для {b} верхняя граница {hi2}; складываем границы: {shi}; сравниваем с {bound}; {shi} < {bound}, условие доказано",
]


def _ctx(rng):
    lo1, hi1, lo2, hi2, bound = rng.choice(RANGES)
    a, b = rng.choice(NAMES)
    shi = hi1 + hi2
    return dict(
        a=a, b=b, lo1=lo1, hi1=hi1, lo2=lo2, hi2=hi2, bound=bound,
        slo=lo1 + lo2, shi=shi,
        n=rng.choice([3, 5, 10, 20, 50, 100, 1000]),
        wrong=rng.randint(hi1, shi),   # заведомо неверная граница суммы
    )


def make_hard(templates, count, seed):
    rng = random.Random(seed)
    seen, out = set(), []
    for _ in range(count * 50):
        if len(out) >= count:
            break
        line = rng.choice(templates).format(**_ctx(rng))
        if line not in seen:
            seen.add(line)
            out.append(line)
    return out


def build_approve(total=510, n_hard=100):
    rng = random.Random(42)
    seen, out = set(), []
    for line in APPROVE_BASE + make_hard(HARD_APPROVE, n_hard, 7):
        if line not in seen:
            seen.add(line)
            out.append(line)

    combos = [(t, r, n) for t in APPROVE_TEMPLATES for r in RANGES for n in NAMES]
    rng.shuffle(combos)
    for tmpl, (lo1, hi1, lo2, hi2, bound), (a, b) in combos:
        if len(out) >= total:
            break
        line = tmpl.format(a=a, b=b, lo1=lo1, hi1=hi1, lo2=lo2, hi2=hi2,
                           bound=bound, slo=lo1 + lo2, shi=hi1 + hi2)
        if line not in seen:
            seen.add(line)
            out.append(line)
    return out[:total]


def build_veto(total=510, n_hard=150):
    rng = random.Random(1)
    hard = make_hard(HARD_VETO, n_hard, 11)
    seen = set(hard)
    base = []
    for line in VETO_BASE:
        if line not in seen:
            seen.add(line)
            base.append(line)
    rng.shuffle(base)

    out = hard + base[: total - len(hard)]
    if len(out) < total:                      # добиваем суффиксами
        extra = [b + s for b in base for s in VETO_SUFFIXES if s]
        rng.shuffle(extra)
        for line in extra:
            if len(out) >= total:
                break
            if line not in seen:
                seen.add(line)
                out.append(line)
    return out[:total]

import os

import os


def load_manual(path="manual.csv", repeat=5):
    rows = []
    if os.path.exists(path):
        with open(path, encoding="utf-8", newline="") as f:
            for r in csv.reader(f):
                if len(r) >= 2 and r[0] in ("0", "1"):
                    rows += [(int(r[0]), r[1])] * repeat
    return rows


def main():
    approve = build_approve()
    veto = build_veto()
    n = min(len(approve), len(veto))
    approve, veto = approve[:n], veto[:n]

    rows = [(1, t) for t in approve] + [(0, t) for t in veto]

    manual = load_manual()
    rows += manual
    print(f"[gen] manual rows: {len(manual)}")

    random.Random(2026).shuffle(rows)

    with open("train.csv", "w", newline="", encoding="utf-8") as f:
        w = csv.writer(f, quoting=csv.QUOTE_MINIMAL)
        w.writerow(["label", "text"])
        for label, text in rows:
            w.writerow([label, text])

    print(f"[gen] wrote {len(rows)} rows to train.csv")
    print(f"[gen] approve={len(approve)}, veto={len(veto)}")


if __name__ == "__main__":
    main()