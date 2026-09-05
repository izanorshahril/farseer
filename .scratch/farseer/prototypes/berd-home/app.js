const root = document.documentElement;
const shell = document.querySelector('#shell');
const canvas = document.querySelector('#canvas');
const sidebar = document.querySelector('#sidebar');
const prompt = document.querySelector('#prompt');
const palette = document.querySelector('#commandPalette');
const searchInput = document.querySelector('#searchInput');
const toast = document.querySelector('#toast');
const clockWidget = document.querySelector('#clockWidget');
const clockToggle = document.querySelector('#clockToggle');
let toastTimer;
let arrange = true;

function showToast(message) {
  toast.textContent = message;
  toast.classList.add('show');
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => toast.classList.remove('show'), 2600);
}

function setClockVisible(visible) {
  clockWidget.hidden = !visible;
  clockToggle.setAttribute('aria-pressed', String(visible));
  const action = visible ? 'Hide' : 'Show';
  clockToggle.setAttribute('aria-label', `${action} clock widget`);
  clockToggle.title = `${action} clock widget`;
  showToast(`Clock widget ${visible ? 'shown' : 'hidden'}`);
}

function updateClock() {
  const now = new Date();
  const value = new Intl.DateTimeFormat([], { hour: '2-digit', minute: '2-digit' }).format(now);
  const clock = document.querySelector('#clock');
  clock.textContent = value;
  clock.dateTime = now.toISOString();
  const hour = now.getHours();
  document.querySelector('#greeting').textContent = hour < 12 ? 'Good morning' : hour < 18 ? 'Good afternoon' : 'Good evening';
}

function openPalette() {
  palette.hidden = false;
  requestAnimationFrame(() => searchInput.focus());
}

function closePalette() {
  palette.hidden = true;
  document.querySelector('#searchButton').focus();
}

function selectProject(name) {
  document.querySelectorAll('[data-project]').forEach((button) => button.classList.toggle('selected', button.dataset.project === name));
  document.querySelector('#projectContextName').textContent = name;
  showToast(`Project context set to ${name}`);
}

function pinPosition(pin) {
  const frame = canvas.getBoundingClientRect();
  const box = pin.getBoundingClientRect();
  return { left: box.left - frame.left, top: box.top - frame.top };
}

document.querySelector('#collapseSidebar').addEventListener('click', () => shell.classList.add('collapsed'));
document.querySelector('#openSidebar').addEventListener('click', () => shell.classList.add('sidebar-open'));
document.querySelector('#searchButton').addEventListener('click', openPalette);
document.querySelector('[data-close]').addEventListener('click', closePalette);
document.querySelector('#hideClock').addEventListener('click', () => {
  setClockVisible(false);
  clockToggle.focus();
});
clockToggle.addEventListener('click', () => setClockVisible(clockWidget.hidden));

document.querySelector('#themeButton').addEventListener('click', () => {
  const dark = root.dataset.theme === 'dark';
  root.dataset.theme = dark ? 'light' : 'dark';
  showToast(`${dark ? 'Light' : 'Dark'} appearance enabled`);
});

document.querySelectorAll('.nav-item,.quota-line').forEach((button) => {
  button.addEventListener('click', () => {
    const view = button.dataset.view;
    if (view === 'Home') return;
    shell.classList.remove('sidebar-open');
    showToast(`${view} stays a home entry point in this prototype`);
  });
});

document.querySelectorAll('[data-project]').forEach((button) => {
  button.addEventListener('click', (event) => {
    event.stopPropagation();
    selectProject(button.dataset.project);
    shell.classList.remove('sidebar-open');
  });
});

document.querySelectorAll('[data-action]').forEach((button) => {
  button.addEventListener('click', () => showToast(`${button.dataset.action} - detail view comes after home approval`));
});

document.querySelector('#arrangeButton').addEventListener('click', (event) => {
  arrange = !arrange;
  event.currentTarget.setAttribute('aria-pressed', String(arrange));
  showToast(arrange ? 'Pins unlocked - drag to arrange' : 'Pins locked in place');
});

document.querySelector('#recenterButton').addEventListener('click', () => {
  document.querySelectorAll('[data-pin]').forEach((pin) => {
    pin.style.left = 'var(--x)';
    pin.style.top = 'var(--y)';
  });
  showToast('Home pins recentered');
});

document.querySelectorAll('[data-pin]').forEach((pin) => {
  pin.addEventListener('pointerdown', (event) => {
    if (!arrange || event.button !== 0 || event.target.closest('button')) return;
    const frame = canvas.getBoundingClientRect();
    const box = pin.getBoundingClientRect();
    const offsetX = event.clientX - box.left;
    const offsetY = event.clientY - box.top;
    pin.style.left = `${box.left - frame.left}px`;
    pin.style.top = `${box.top - frame.top}px`;
    pin.classList.add('dragging');
    canvas.classList.add('drag-active');
    pin.setPointerCapture(event.pointerId);

    const move = (moveEvent) => {
      const maxLeft = frame.width - box.width - 12;
      const maxTop = frame.height - box.height - 12;
      pin.style.left = `${Math.max(12, Math.min(maxLeft, moveEvent.clientX - frame.left - offsetX))}px`;
      pin.style.top = `${Math.max(12, Math.min(maxTop, moveEvent.clientY - frame.top - offsetY))}px`;
    };
    const finish = () => {
      pin.classList.remove('dragging');
      canvas.classList.remove('drag-active');
      pin.removeEventListener('pointermove', move);
      pin.removeEventListener('pointerup', finish);
      pin.removeEventListener('pointercancel', finish);
    };
    pin.addEventListener('pointermove', move);
    pin.addEventListener('pointerup', finish);
    pin.addEventListener('pointercancel', finish);
  });

  pin.addEventListener('keydown', (event) => {
    if (!arrange || !['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight'].includes(event.key)) return;
    event.preventDefault();
    const frame = canvas.getBoundingClientRect();
    const box = pin.getBoundingClientRect();
    const current = pinPosition(pin);
    const delta = event.shiftKey ? 24 : 8;
    const dx = event.key === 'ArrowLeft' ? -delta : event.key === 'ArrowRight' ? delta : 0;
    const dy = event.key === 'ArrowUp' ? -delta : event.key === 'ArrowDown' ? delta : 0;
    pin.style.left = `${Math.max(12, Math.min(frame.width - box.width - 12, current.left + dx))}px`;
    pin.style.top = `${Math.max(12, Math.min(frame.height - box.height - 12, current.top + dy))}px`;
  });
});

prompt.addEventListener('input', () => {
  prompt.style.height = '38px';
  prompt.style.height = `${Math.min(prompt.scrollHeight, 84)}px`;
});

document.querySelector('#composer').addEventListener('submit', (event) => {
  event.preventDefault();
  const value = prompt.value.trim();
  if (!value) {
    prompt.focus();
    showToast('Write an instruction for the top manager');
    return;
  }
  showToast('Prototype only - no request was sent');
  prompt.value = '';
  prompt.style.height = '38px';
});

document.addEventListener('keydown', (event) => {
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k') {
    event.preventDefault();
    openPalette();
  }
  if ((event.ctrlKey || event.metaKey) && event.key === ',') {
    event.preventDefault();
    showToast('Settings stays a home entry point in this prototype');
  }
  if (event.key === 'Escape') {
    if (!palette.hidden) closePalette();
    shell.classList.remove('sidebar-open');
  }
});

updateClock();
setInterval(updateClock, 15000);
