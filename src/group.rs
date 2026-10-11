//! Группа счётчиков и результаты измерений.

use std::collections::HashMap;
use std::fmt;
use std::io;
use std::ops::Index;
use std::sync::Arc;

use perf_event::events::Event;
use perf_event::{Builder, Counter, Group};

use crate::config::GroupConfig;

/// Пользовательская доработка `Builder`а перед созданием счётчика.
///
/// Хранится в [`Entry`], чтобы при пересборке шарда можно было воссоздать
/// счётчик с теми же настройками. `Arc` — чтобы настройку можно было
/// клонировать в новый шард.
type Tune = Arc<dyn for<'b> Fn(&mut Builder<'b>) + 'static>;

/// Один добавленный в шард счётчик.
struct Entry {
    name: String,
    event: Event,
    tune: Option<Tune>,
    counter: Counter,
}

/// Физическая группа ядра (`perf_event::Group`) + её счётчики.
///
/// Аппаратных регистров мало, поэтому счётчики распределяются по нескольким
/// шардам: каждый шард измеряется своим прогоном.
struct Shard {
    group: Group,
    entries: Vec<Entry>,
    /// `time_enabled`/`time_running` на момент последнего сброса счётчиков.
    ///
    /// Ядро **не** обнуляет эти времена при `ioctl(RESET)` — они копятся
    /// с момента открытия, поэтому результаты вычитаются из базовой линии.
    baseline_enabled: u64,
    baseline_running: u64,
}

/// Группа именованных счётчиков, которые включаются/выключаются атомарно.
///
/// ```
/// use perfrs::{GroupConfig, PerfGroup};
/// use perfrs::events::Hardware;
///
/// # fn demo() -> std::io::Result<()> {
/// let config = GroupConfig::new().exclude_kernel(true);
/// let mut group = PerfGroup::with_config(config)?;
/// group.counter("cycles", Hardware::CPU_CYCLES)?;
/// group.counter("instructions", Hardware::INSTRUCTIONS)?;
///
/// let (_, samples) = group.measure(|| (0..10_000).sum::<u64>())?;
/// let ipc = samples.ratio("instructions", "cycles").unwrap_or(f64::NAN);
/// println!("IPC = {ipc:.2}");
/// # Ok(()) }
/// ```
///
/// ## Если счётчиков больше, чем регистров
///
/// Аппаратных регистров обычно 4–8 на группу. Счётчик, который не влез,
/// не вызывает ошибку: он попадает в **следующий шард**, и [`measure`]
/// запускает измеряемый код сначала без него (основная группа), а потом
/// с ним (следующий шард). Значения всех счётчиков сливаются в один
/// [`Samples`].
///
/// [`measure`]: PerfGroup::measure
pub struct PerfGroup {
    config: GroupConfig,
    shards: Vec<Shard>,
}

impl PerfGroup {
    /// Группа с параметрами по умолчанию.
    pub fn new() -> io::Result<Self> {
        Self::with_config(GroupConfig::new())
    }

    /// Группа с явно заданными параметрами (см. [`GroupConfig`]).
    pub fn with_config(config: GroupConfig) -> io::Result<Self> {
        Ok(PerfGroup {
            config,
            shards: vec![Shard {
                group: Group::new()?,
                entries: Vec::new(),
                baseline_enabled: 0,
                baseline_running: 0,
            }],
        })
    }

    /// Группа, собранная из готового списка «(имя, событие)».
    ///
    /// ```
    /// use perfrs::{GroupConfig, PerfGroup};
    /// use perfrs::events::Hardware;
    ///
    /// # fn demo() -> std::io::Result<()> {
    /// let counters = [
    ///     ("cycles", Hardware::CPU_CYCLES.into()),
    ///     ("instructions", Hardware::INSTRUCTIONS.into()),
    /// ];
    /// let group = PerfGroup::from_counters(&counters, GroupConfig::new())?;
    /// # drop(group);
    /// # Ok(()) }
    /// ```
    pub fn from_counters(counters: &[(&str, Event)], config: GroupConfig) -> io::Result<Self> {
        let mut group = Self::with_config(config)?;
        group.counters(counters)?;
        Ok(group)
    }

    /// Параметры, с которыми создана группа.
    pub fn config(&self) -> &GroupConfig {
        &self.config
    }

    /// Имена добавленных счётчиков в порядке добавления.
    pub fn names(&self) -> impl Iterator<Item = &str> + '_ {
        self.shards
            .iter()
            .flat_map(|shard| shard.entries.iter().map(|entry| entry.name.as_str()))
    }

    /// Количество счётчиков во всех шардах.
    pub fn len(&self) -> usize {
        self.shards.iter().map(|shard| shard.entries.len()).sum()
    }

    /// `true`, если счётчиков ещё нет.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Сколько прогонов сделает [`measure`](Self::measure): по одному на шард.
    ///
    /// `1` — все счётчики влезли в одну аппаратенную группу.
    pub fn passes(&self) -> usize {
        self.shards.len()
    }

    /// Имена счётчиков, которые не влезли в основную группу и поэтому
    /// измеряются отдельным(и) прогоном(ами) после основного.
    pub fn overflow(&self) -> impl Iterator<Item = &str> + '_ {
        self.shards[1..]
            .iter()
            .flat_map(|shard| shard.entries.iter().map(|entry| entry.name.as_str()))
    }

    /// Добавить именованный счётчик события `kind`.
    ///
    /// Имя должно быть уникальным — по нему потом достаются значения из
    /// [`Samples`]. Если событие не поддерживается машиной, возвращается
    /// ошибка с его именем. Если счётчик не влез в аппаратенную группу,
    /// он попадает в следующий шард и будет измерен отдельным прогоном.
    pub fn counter<K: Into<Event>>(
        &mut self,
        name: impl Into<String>,
        kind: K,
    ) -> io::Result<&mut Self> {
        self.add_event(name.into(), kind.into(), None, true)?;
        Ok(self)
    }

    /// Добавить счётчик, если событие поддерживается, иначе вернуть `Ok(false)`.
    ///
    /// Не влезший в группу счётчик **не** считается неудачей — он просто
    /// уходит в следующий шард (см. [`overflow`](Self::overflow)).
    ///
    /// ```
    /// use perfrs::PerfGroup;
    /// use perfrs::events::Hardware;
    ///
    /// # fn demo() -> std::io::Result<()> {
    /// let mut group = PerfGroup::new()?;
    /// let has_stall = group.try_counter("stall", Hardware::STALLED_CYCLES_FRONTEND)?;
    /// println!("stalled counters: {has_stall}");
    /// # Ok(()) }
    /// ```
    pub fn try_counter<K: Into<Event>>(
        &mut self,
        name: impl Into<String>,
        kind: K,
    ) -> io::Result<bool> {
        self.add_event(name.into(), kind.into(), None, false)
    }

    /// Добавить счётчик с дополнительной настройкой `Builder`а.
    ///
    /// Замыкание получает `Builder` уже с применённой [`GroupConfig`],
    /// группой и событием — можно докрутить что угодно. Требование:
    /// замыкание должно быть `Fn` (оно хранится в группе и вызывается
    /// повторно при пересборке шарда).
    ///
    /// ```
    /// use perfrs::PerfGroup;
    /// use perfrs::events::Hardware;
    ///
    /// # fn demo() -> std::io::Result<()> {
    /// let mut group = PerfGroup::new()?;
    /// group.counter_with("cycles", Hardware::CPU_CYCLES, |b| {
    ///     b.pinned(true);
    /// })?;
    /// # drop(group);
    /// # Ok(()) }
    /// ```
    pub fn counter_with<K, F>(
        &mut self,
        name: impl Into<String>,
        kind: K,
        f: F,
    ) -> io::Result<&mut Self>
    where
        K: Into<Event>,
        F: for<'b> Fn(&mut Builder<'b>) + 'static,
    {
        let tune: Tune = Arc::new(f);
        self.add_event(name.into(), kind.into(), Some(tune), true)?;
        Ok(self)
    }

    /// Добавить несколько счётчиков из списка «(имя, событие)».
    pub fn counters(&mut self, counters: &[(&str, Event)]) -> io::Result<&mut Self> {
        for (name, kind) in counters {
            self.counter(*name, kind.clone())?;
        }
        Ok(self)
    }

    /// Включить все счётчики всех шардов атомарно.
    pub fn enable(&mut self) -> io::Result<()> {
        for shard in &mut self.shards {
            shard.group.enable()?;
        }
        Ok(())
    }

    /// Выключить все счётчики всех шардов атомарно.
    pub fn disable(&mut self) -> io::Result<()> {
        for shard in &mut self.shards {
            shard.group.disable()?;
        }
        Ok(())
    }

    /// Обнулить все счётчики всех шардов атомарно.
    ///
    /// Время работы (`time_enabled`/`time_running`) привязывается к новому
    /// отрезку, чтобы последующий [`read`](Self::read) относился только
    /// к этому замеру.
    pub fn reset(&mut self) -> io::Result<()> {
        for shard in &mut self.shards {
            reset_shard(shard)?;
        }
        Ok(())
    }

    /// Прогреть то, что будет измеряться: частоту CPU, предиктор ветвлений,
    /// кэши и аллокатор — без записи счётчиков.
    ///
    /// Короткие замеры (тысячи инструкций) без прогрева дают сильно
    /// заниженный IPC: почти все циклы уходят на холодный пайплайн.
    ///
    /// ```
    /// use perfrs::PerfGroup;
    /// use perfrs::events::Hardware;
    ///
    /// # fn demo() -> std::io::Result<()> {
    /// let mut group = PerfGroup::new()?;
    /// group.counter("cycles", Hardware::CPU_CYCLES)?;
    ///
    /// let work = || (0..10_000).fold(0u64, |a, b| a.wrapping_add(b));
    /// group.warmup(work)?;
    /// let (value, samples) = group.measure(work)?;
    /// # drop((value, samples));
    /// # Ok(()) }
    /// ```
    pub fn warmup<F, R>(&mut self, f: F) -> io::Result<()>
    where
        F: FnOnce() -> R,
    {
        for shard in &mut self.shards {
            shard.group.enable()?;
        }
        let _ = f();
        self.disable()?;
        self.reset()?;
        Ok(())
    }

    /// Измерить `f` всеми счётчиками.
    ///
    /// Шарды измеряются **последовательно**: сперва основная группа
    /// (счётчики, которые влезли), потом, если есть счётчики «сверх лимита»,
    /// `f` запускается ещё раз для каждого следующего шарда — то есть
    /// сначала без них, а потом с ними.
    ///
    /// Возвращает результат **первого** прогона (основная группа) и
    /// [`Samples`] со значениями всех счётчиков.
    ///
    /// Внимание: счётчики из разных прогонов получены из **разных запусков**
    /// `f`. Отношения внутри одного шарда точны, а между шардами
    /// (например, `stalled_frontend / cycles`) — приблизительны: работа
    /// повторяется, но может немного отличаться от прогона до прогона.
    ///
    /// ```
    /// use perfrs::PerfGroup;
    /// use perfrs::events::Hardware;
    ///
    /// # fn demo() -> std::io::Result<()> {
    /// let mut group = PerfGroup::new()?;
    /// group.counter("cycles", Hardware::CPU_CYCLES)?;
    /// group.counter("stall", Hardware::STALLED_CYCLES_FRONTEND)?;
    ///
    /// // Может выполниться дважды, если stall не влез в одну группу.
    /// let (value, samples) = group.measure(|| (0..1000).sum::<u64>())?;
    /// println!("{value}, прогонов: {}", samples.passes());
    /// # Ok(()) }
    /// ```
    pub fn measure<F, R>(&mut self, mut f: F) -> io::Result<(R, Samples)>
    where
        F: FnMut() -> R,
    {
        let mut first: Option<R> = None;
        let mut merged: Option<Samples> = None;

        for index in 0..self.shards.len() {
            let shard = &mut self.shards[index];
            reset_shard(shard)?;
            shard.group.enable()?;

            let value = {
                let guard = MeasureGuard(&mut shard.group);
                let value = f();
                drop(guard);
                value
            };

            let samples = read_shard(shard)?;
            if first.is_none() {
                first = Some(value);
            }
            merged = Some(match merged {
                Some(mut merged) => {
                    merged.merge(samples);
                    merged
                }
                None => samples,
            });
        }

        let mut samples = merged.unwrap_or_else(|| Samples::new(0, 0));
        samples.set_passes(self.shards.len());
        let value = first.expect("у группы всегда есть хотя бы один шард");
        Ok((value, samples))
    }

    /// Как [`measure`](Self::measure), но `f` может вернуть ошибку.
    ///
    /// При ошибке в каком-либо прогоне счётчики этого шарда выключаются,
    /// а ошибка возвращается вызывающему.
    pub fn try_measure<F, R>(&mut self, mut f: F) -> io::Result<(R, Samples)>
    where
        F: FnMut() -> io::Result<R>,
    {
        let mut first: Option<R> = None;
        let mut merged: Option<Samples> = None;

        for index in 0..self.shards.len() {
            let shard = &mut self.shards[index];
            reset_shard(shard)?;
            shard.group.enable()?;

            let value = {
                let guard = MeasureGuard(&mut shard.group);
                let value = f()?;
                drop(guard);
                value
            };

            let samples = read_shard(shard)?;
            if first.is_none() {
                first = Some(value);
            }
            merged = Some(match merged {
                Some(mut merged) => {
                    merged.merge(samples);
                    merged
                }
                None => samples,
            });
        }

        let mut samples = merged.unwrap_or_else(|| Samples::new(0, 0));
        samples.set_passes(self.shards.len());
        let value = first.expect("у группы всегда есть хотя бы один шард");
        Ok((value, samples))
    }

    /// Прочитать текущие значения всех счётчиков всех шардов.
    ///
    /// Значения накопительные с последнего [`measure`](Self::measure):
    /// счётчики живут в ядре и не обнуляются между чтениями.
    pub fn read(&mut self) -> io::Result<Samples> {
        let mut merged: Option<Samples> = None;

        for shard in &mut self.shards {
            let samples = read_shard(shard)?;
            merged = Some(match merged {
                Some(mut merged) => {
                    merged.merge(samples);
                    merged
                }
                None => samples,
            });
        }

        let mut samples = merged.unwrap_or_else(|| Samples::new(0, 0));
        samples.set_passes(self.shards.len());
        Ok(samples)
    }

    fn ensure_unique(&self, name: &str) -> io::Result<()> {
        if self.names().any(|existing| existing == name) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("счётчик `{name}` уже добавлен в группу"),
            ));
        }
        Ok(())
    }

    fn add_event(
        &mut self,
        name: String,
        event: Event,
        tune: Option<Tune>,
        missing_is_error: bool,
    ) -> io::Result<bool> {
        self.ensure_unique(&name)?;

        // Сначала проверяем событие отдельным счётчиком (не в группе):
        // так неподдерживаемое событие не потащит за собой ошибки.
        if let Err(e) = self.probe(&name, &event, &tune) {
            if missing_is_error {
                return Err(e);
            }
            return Ok(false);
        }

        // Пытаемся влезть в существующие шарды: кандидат строится в
        // отдельной группе, поэтому живые шарды не портятся при неудаче.
        // TODO OPTIMIZATION
        for index in 0..self.shards.len() {
            if let Some(candidate) = self.try_grow(index, &name, &event, &tune)? {
                let (group, entries) = candidate;
                self.shards[index].group = group;
                self.shards[index].entries = entries;
                return Ok(true);
            }
        }

        // Никуда не влезает — заводим новый шард (отдельный прогон).
        let shard = self.try_new_shard(&name, &event, &tune)?;
        self.shards.push(shard);
        Ok(true)
    }

    fn probe(&self, name: &str, event: &Event, tune: &Option<Tune>) -> io::Result<()> {
        let mut builder = self.config.apply(Builder::new());
        builder = builder.kind(event.clone());
        if let Some(tune) = tune {
            tune(&mut builder);
        }
        builder.build().map(|_| ()).map_err(|e| {
            io::Error::new(e.kind(), format!("счётчик `{name}` не поддерживается: {e}"))
        })
    }

    /// Попытка собрать шард `index` вместе с новым счётчиком в **новой**
    /// группе. `Ok(None)` — не влезло (или иная нехватка ресурсов).
    fn try_grow(
        &self,
        index: usize,
        name: &str,
        event: &Event,
        tune: &Option<Tune>,
    ) -> io::Result<Option<(Group, Vec<Entry>)>> {
        let shard = &self.shards[index];
        let mut group = Group::new()?;
        let mut entries = Vec::with_capacity(shard.entries.len() + 1);

        for entry in &shard.entries {
            match build_into(&mut group, &self.config, &entry.event, &entry.tune) {
                Ok(counter) => entries.push(Entry {
                    name: entry.name.clone(),
                    event: entry.event.clone(),
                    tune: entry.tune.clone(),
                    counter,
                }),
                Err(_) => return Ok(None),
            }
        }

        match build_into(&mut group, &self.config, event, tune) {
            Ok(counter) => {
                entries.push(Entry {
                    name: name.to_string(),
                    event: event.clone(),
                    tune: tune.clone(),
                    counter,
                });
                Ok(Some((group, entries)))
            }
            Err(_) => Ok(None),
        }
    }

    /// Новый шард, содержащий только данный счётчик.
    fn try_new_shard(&self, name: &str, event: &Event, tune: &Option<Tune>) -> io::Result<Shard> {
        let mut group = Group::new()?;
        let counter = build_into(&mut group, &self.config, event, tune).map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("не удалось создать отдельный прогон для `{name}`: {e}"),
            )
        })?;

        Ok(Shard {
            group,
            baseline_enabled: 0,
            baseline_running: 0,
            entries: vec![Entry {
                name: name.to_string(),
                event: event.clone(),
                tune: tune.clone(),
                counter,
            }],
        })
    }
}

/// Создать счётчик `event` внутри `group`, применяя `config` и `tune`.
fn build_into(
    group: &mut Group,
    config: &GroupConfig,
    event: &Event,
    tune: &Option<Tune>,
) -> io::Result<Counter> {
    let mut builder = config.apply(Builder::new());
    builder = builder.group(group);
    builder = builder.kind(event.clone());
    if let Some(tune) = tune {
        tune(&mut builder);
    }
    builder.build()
}

/// Обнулить счётчики шарда и запомнить базовую линию времени.
fn reset_shard(shard: &mut Shard) -> io::Result<()> {
    let counts = shard.group.read()?;
    shard.baseline_enabled = counts.time_enabled();
    shard.baseline_running = counts.time_running();
    shard.group.reset()
}

/// Прочитать один шард, масштабируя значения на время работы.
fn read_shard(shard: &mut Shard) -> io::Result<Samples> {
    let counts = shard.group.read()?;
    // Времена в ядре копятся с открытия счётчика — берём только текущий
    // отрезок (с последнего сброса), как и счётчики.
    let enabled = counts.time_enabled().saturating_sub(shard.baseline_enabled);
    let running = counts.time_running().saturating_sub(shard.baseline_running);
    let scale = if running == 0 {
        0.0
    } else {
        enabled as f64 / running as f64
    };

    let mut samples = Samples::new(enabled, running);
    for entry in &shard.entries {
        let raw = counts.get(&entry.counter).copied().unwrap_or(0);
        let scaled = if scale > 1.0 {
            (raw as f64 * scale) as u64
        } else {
            raw
        };
        samples.push(entry.name.clone(), scaled, raw);
    }
    Ok(samples)
}

impl fmt::Debug for PerfGroup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PerfGroup")
            .field("config", &self.config)
            .field("counters", &self.names().collect::<Vec<_>>())
            .field("passes", &self.passes())
            .finish()
    }
}

/// Выключает шард даже при панике внутри измеряемого кода.
struct MeasureGuard<'a>(&'a mut Group);

impl Drop for MeasureGuard<'_> {
    fn drop(&mut self) {
        let _ = self.0.disable();
    }
}

/// Снимок значений счётчиков одного или нескольких прогонов.
///
/// Значения масштабированы на `time_enabled / time_running`, поэтому
/// корректны даже когда ядро «дробило» счётчики между группами.
#[derive(Clone, Debug, Default)]
pub struct Samples {
    values: HashMap<String, u64>,
    raw: HashMap<String, u64>,
    order: Vec<String>,
    time_enabled: u64,
    time_running: u64,
    passes: usize,
}

impl Samples {
    fn new(time_enabled: u64, time_running: u64) -> Self {
        Samples {
            values: HashMap::new(),
            raw: HashMap::new(),
            order: Vec::new(),
            time_enabled,
            time_running,
            passes: 1,
        }
    }

    fn push(&mut self, name: String, value: u64, raw: u64) {
        self.order.push(name.clone());
        self.values.insert(name.clone(), value);
        self.raw.insert(name, raw);
    }

    /// Присоединить результаты ещё одного прогона.
    fn merge(&mut self, other: Samples) {
        for name in other.order {
            let value = other.values[&name];
            let raw = other.raw[&name];
            self.push(name, value, raw);
        }
        self.time_enabled += other.time_enabled;
        self.time_running += other.time_running;
        self.passes = self.passes.max(other.passes);
    }

    fn set_passes(&mut self, passes: usize) {
        self.passes = passes;
    }

    /// Масштабированное значение счётчика.
    pub fn get(&self, name: &str) -> Option<u64> {
        self.values.get(name).copied()
    }

    /// Значение счётчика, как его вернул ядро (без масштабирования).
    pub fn raw(&self, name: &str) -> Option<u64> {
        self.raw.get(name).copied()
    }

    /// Есть ли счётчик с таким именем.
    pub fn contains(&self, name: &str) -> bool {
        self.values.contains_key(name)
    }

    /// Имена счётчиков в порядке добавления.
    pub fn names(&self) -> &[String] {
        &self.order
    }

    /// Сколько прогонов вошло в этот снимок (по одному на шард).
    ///
    /// Больше единицы означает, что часть счётчиков не влезала в одну
    /// аппаратенную группу и измерялась отдельным запуском `f`.
    pub fn passes(&self) -> usize {
        self.passes
    }

    /// Итератор `(имя, значение)` в порядке добавления счётчиков.
    pub fn iter(&self) -> impl Iterator<Item = (&str, u64)> + '_ {
        self.order
            .iter()
            .map(|name| (name.as_str(), self.values[name]))
    }

    /// Отношение `numerator / denominator` (например, misses/accesses).
    ///
    /// `None`, если счётчика нет или знаменатель равен нулю.
    pub fn ratio(&self, numerator: &str, denominator: &str) -> Option<f64> {
        let den = self.get(denominator)?;
        if den == 0 {
            return None;
        }
        Some(self.get(numerator)? as f64 / den as f64)
    }

    /// Отношение в процентах: `numerator / denominator * 100`.
    pub fn percent(&self, numerator: &str, denominator: &str) -> Option<f64> {
        self.ratio(numerator, denominator).map(|r| r * 100.0)
    }

    /// Насколько группа реально считала: `time_running / time_enabled`.
    ///
    /// Значение `1.0` означает, что аппаратных регистров хватило на всё.
    pub fn duty_cycle(&self) -> f64 {
        if self.time_enabled == 0 {
            return 0.0;
        }
        self.time_running as f64 / self.time_enabled as f64
    }

    /// Наносекунды, в течение которых группа была включена.
    pub fn time_enabled(&self) -> u64 {
        self.time_enabled
    }

    /// Наносекунды, в течение которых группа реально считала.
    pub fn time_running(&self) -> u64 {
        self.time_running
    }

    /// `true`, если счётчики пришлось масштабировать (ядро делило регистры).
    pub fn scaled(&self) -> bool {
        self.time_running < self.time_enabled
    }
}

impl Index<&str> for Samples {
    type Output = u64;

    fn index(&self, name: &str) -> &u64 {
        self.values
            .get(name)
            .expect("нет счётчика с таким именем в Samples")
    }
}

impl fmt::Display for Samples {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let width = self.order.iter().map(String::len).max().unwrap_or(0);
        for name in &self.order {
            writeln!(f, "{name:width$}  {:>15}", self.values[name])?;
        }
        if self.passes > 1 {
            writeln!(
                f,
                "/* измерено за {} прогонов: часть счётчиков не влезала \
                 в одну аппаратенную группу */",
                self.passes
            )?;
        }
        if self.scaled() {
            writeln!(
                f,
                "/* счётчики масштабированы: duty cycle {:.1}% */",
                self.duty_cycle() * 100.0
            )?;
        }
        if self.time_enabled > 0 && self.time_enabled < 1_000_000 {
            writeln!(
                f,
                "/* замер короткий ({:.2} мс): пайплайн, предиктор и кэши \
                 холодные — IPC может быть сильно занижен; делайте прогон \
                 длиннее или используйте `warmup` */",
                self.time_enabled as f64 / 1_000_000.0
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Samples {
        let mut samples = Samples::new(200, 100);
        samples.push("access".into(), 2000, 1000);
        samples.push("miss".into(), 500, 250);
        samples
    }

    #[test]
    fn ratio_and_percent() {
        let samples = sample();
        assert_eq!(samples.ratio("miss", "access"), Some(0.25));
        assert_eq!(samples.percent("miss", "access"), Some(25.0));
        assert_eq!(samples.ratio("miss", "unknown"), None);
        assert_eq!(samples.ratio("unknown", "access"), None);
    }

    #[test]
    fn scaling_is_applied() {
        let samples = sample();
        assert_eq!(samples.get("access"), Some(2000));
        assert_eq!(samples.raw("access"), Some(1000));
        assert!(samples.scaled());
        assert_eq!(samples.duty_cycle(), 0.5);
    }

    #[test]
    fn iteration_keeps_order() {
        let samples = sample();
        let names: Vec<&str> = samples.iter().map(|(name, _)| name).collect();
        assert_eq!(names, ["access", "miss"]);
        assert_eq!(samples["miss"], 500);
    }

    #[test]
    fn merge_appends_counters_and_sums_time() {
        let mut first = sample();
        let mut second = Samples::new(50, 50);
        second.push("stall".into(), 77, 77);
        second.set_passes(2);

        first.merge(second);

        let names: Vec<&str> = first.iter().map(|(name, _)| name).collect();
        assert_eq!(names, ["access", "miss", "stall"]);
        assert_eq!(first["stall"], 77);
        assert_eq!(first.time_enabled(), 250);
        assert_eq!(first.time_running(), 150);
        assert_eq!(first.passes(), 2);
    }
}
