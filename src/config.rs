//! Параметры группы счётчиков.

use perf_event::Builder;

/// Кого наблюдаем: процесс, конкретный pid или «все процессы».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Observe {
    /// Текущий процесс (по умолчанию).
    SelfProcess,
    /// Процесс с указанным pid (нужен `CAP_SYS_PTRACE`).
    Pid(i32),
    /// Любой процесс — только вместе с [`GroupConfig::one_cpu`].
    Any,
}

/// Параметры, применяемые ко всем счётчикам группы.
///
/// Все сеттеры принимают значение аргументом и возвращают `Self`,
/// поэтому параметры удобно задавать «в цепочке»:
///
/// ```
/// use perfrs::GroupConfig;
///
/// let config = GroupConfig::new()
///     .observe_pid(1234)
///     .one_cpu(2)
///     .exclude_user(true)
///     .include_kernel();
/// # drop(config);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupConfig {
    /// Кого наблюдаем.
    pub observe: Observe,
    /// `None` — любой CPU, `Some(n)` — только ядро `n`.
    pub cpu: Option<usize>,
    /// Исключить события в userspace.
    pub exclude_user: bool,
    /// Исключить события в kernel (по умолчанию `true`).
    pub exclude_kernel: bool,
    /// Исключить события гипервизора (по умолчанию `true`).
    pub exclude_hv: bool,
    /// Исключить события idle-задач (только для software-событий).
    pub exclude_idle: bool,
    /// Требовать выделения аппаратенного счётчика (иначе — ошибка).
    pub pinned: bool,
    /// Группа не должна делиться с другими группами.
    pub exclusive: bool,
    /// Наследовать счётчики дочерним потокам.
    pub inherit: bool,
}

impl Default for GroupConfig {
    fn default() -> Self {
        GroupConfig {
            observe: Observe::SelfProcess,
            cpu: None,
            exclude_user: false,
            exclude_kernel: true,
            exclude_hv: true,
            exclude_idle: false,
            pinned: false,
            exclusive: false,
            inherit: false,
        }
    }
}

impl GroupConfig {
    /// Конфигурация со значениями по умолчанию.
    pub fn new() -> Self {
        Self::default()
    }

    /// Наблюдать за текущим процессом (по умолчанию).
    pub fn observe_self(mut self) -> Self {
        self.observe = Observe::SelfProcess;
        self
    }

    /// Наблюдать за процессом `pid`.
    pub fn observe_pid(mut self, pid: i32) -> Self {
        self.observe = Observe::Pid(pid);
        self
    }

    /// Наблюдать за всеми процессами (требует [`one_cpu`](Self::one_cpu)).
    pub fn any_pid(mut self) -> Self {
        self.observe = Observe::Any;
        self
    }

    /// Считать только на ядре `cpu`.
    pub fn one_cpu(mut self, cpu: usize) -> Self {
        self.cpu = Some(cpu);
        self
    }

    /// Считать на любом ядре (по умолчанию).
    pub fn any_cpu(mut self) -> Self {
        self.cpu = None;
        self
    }

    /// Считать/не считать события в userspace.
    pub fn exclude_user(mut self, exclude: bool) -> Self {
        self.exclude_user = exclude;
        self
    }

    /// Считать/не считать события в kernel.
    pub fn exclude_kernel(mut self, exclude: bool) -> Self {
        self.exclude_kernel = exclude;
        self
    }

    /// Включить события kernel (=`exclude_kernel(false)`).
    pub fn include_kernel(mut self) -> Self {
        self.exclude_kernel = false;
        self
    }

    /// Считать/не считать события гипервизора.
    pub fn exclude_hv(mut self, exclude: bool) -> Self {
        self.exclude_hv = exclude;
        self
    }

    /// Включить события гипервизора (=`exclude_hv(false)`).
    pub fn include_hv(mut self) -> Self {
        self.exclude_hv = false;
        self
    }

    /// Исключать idle-задачи.
    pub fn exclude_idle(mut self, exclude: bool) -> Self {
        self.exclude_idle = exclude;
        self
    }

    /// Требовать выделенный аппаратенный счётчик.
    pub fn pinned(mut self, pinned: bool) -> Self {
        self.pinned = pinned;
        self
    }

    /// Запретить деление группы с другими.
    pub fn exclusive(mut self, exclusive: bool) -> Self {
        self.exclusive = exclusive;
        self
    }

    /// Наследовать счётчики дочерним потокам.
    ///
    /// Не поддерживается ядром для счётчиков внутри группы — обычно
    /// возвращает ошибку `EINVAL` при создании счётчика.
    pub fn inherit(mut self, inherit: bool) -> Self {
        self.inherit = inherit;
        self
    }

    /// Применить конфигурацию к [`Builder`]у.
    ///
    /// Полезно, если вы собираете счётчики вручную через `perf_event::Builder`.
    pub fn apply<'a>(&self, builder: Builder<'a>) -> Builder<'a> {
        let builder = match self.observe {
            Observe::SelfProcess => builder.observe_self(),
            Observe::Pid(pid) => builder.observe_pid(pid),
            Observe::Any => builder.any_pid(),
        };

        let builder = match self.cpu {
            Some(cpu) => builder.one_cpu(cpu),
            None => builder.any_cpu(),
        };

        let mut builder = builder;
        builder.exclude_user(self.exclude_user);
        builder.exclude_kernel(self.exclude_kernel);
        builder.exclude_hv(self.exclude_hv);
        builder.exclude_idle(self.exclude_idle);
        builder.pinned(self.pinned);
        builder.exclusive(self.exclusive);

        if self.inherit {
            builder.inherit(true);
        }

        builder
    }
}
