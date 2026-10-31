#!/usr/bin/env python3
"""Native NeonTrace GUI acceptance checks. No network probes.

python tools/ui_smoke.py /absolute/path/neontrace --out /tmp/nt-ui-qa
Uses Xvfb, X11/Xtst through ctypes, ImageMagick import, tesseract, dbus-next.
Creates exactly six locale-main screenshots at 680x540, plus English 1280x800,
live light-theme, Settings/targets/details, and actual exported PNG/JSON/CSV.
"""
import argparse
import asyncio
import csv
import ctypes as C
import ctypes.util
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time


def offline_network_guard(output):
    source = output / 'offline-network-guard.c'
    library = output / 'offline-network-guard.so'
    source.write_text(r"""
#define _GNU_SOURCE
#include <dlfcn.h>
#include <sys/socket.h>
#include <stdio.h>
#include <stdlib.h>
#include <errno.h>
int socket(int family, int type, int protocol) {
    const char *audit = getenv("NEONTRACE_NETWORK_AUDIT");
    if (audit && (family == AF_INET || family == AF_INET6)) {
        FILE *file = fopen(audit, "a");
        if (file) { fputs("network_socket_attempt\n", file); fclose(file); }
        errno = EPERM;
        return -1;
    }
    int (*original)(int,int,int) = dlsym(RTLD_NEXT, "socket");
    return original(family,type,protocol);
}
""")
    subprocess.run(['cc','-shared','-fPIC',str(source),'-ldl','-o',str(library)],check=True)
    return library


def portal_main(state_file, output):
    from dbus_next import Variant, Message, MessageType, PropertyAccess
    from dbus_next.aio import MessageBus
    from dbus_next.service import ServiceInterface, method, signal, dbus_property
    state_file, output = Path(state_file), Path(output)
    output.mkdir(parents=True, exist_ok=True)
    scheme = [int(state_file.read_text())]
    class Settings(ServiceInterface):
        def __init__(self): super().__init__('org.freedesktop.portal.Settings')
        @method()
        def Read(self, namespace: 's', key: 's') -> 'v':
            if namespace == 'org.freedesktop.appearance' and key == 'color-scheme':
                return Variant('v', Variant('u', scheme[0]))
            if key == 'enable-animations': return Variant('v', Variant('b', True))
            return Variant('v', Variant('u', 0))
        @method()
        def ReadOne(self, namespace: 's', key: 's') -> 'v':
            return Variant('u', scheme[0] if key == 'color-scheme' else 0)
        @signal()
        def SettingChanged(self, namespace: 's', key: 's', value: 'v') -> 'ssv':
            return [namespace, key, value]
        @dbus_property(access=PropertyAccess.READ)
        def version(self) -> 'u': return 2
    class FileChooser(ServiceInterface):
        def __init__(self): super().__init__('org.freedesktop.portal.FileChooser')
        @method()
        def SaveFile(self, parent_window: 's', title: 's', options: 'a{sv}') -> 'o':
            return '/org/freedesktop/portal/desktop/request/unhandled'
        @dbus_property(access=PropertyAccess.READ)
        def version(self) -> 'u': return 4
    async def run():
        bus = await MessageBus().connect()
        settings = Settings()
        bus.export('/org/freedesktop/portal/desktop', settings)
        bus.export('/org/freedesktop/portal/desktop', FileChooser())
        def dispatch(message):
            if message.message_type != MessageType.METHOD_CALL or message.interface != 'org.freedesktop.portal.FileChooser' or message.member != 'SaveFile': return
            options = message.body[2]
            token = options.get('handle_token', Variant('s', 'qa')).value
            sender = message.sender.lstrip(':').replace('.', '_')
            request = f'/org/freedesktop/portal/desktop/request/{sender}/{token}'
            name = Path(options.get('current_name', Variant('s', 'NeonTrace-QA.data')).value).name
            destination = output / name
            with (output / 'portal-exports.jsonl').open('a') as log:
                log.write(json.dumps({'path': str(destination), 'request': request}) + '\n')
            def response():
                bus.send(Message.new_signal(request, 'org.freedesktop.portal.Request', 'Response', signature='ua{sv}', body=[0, {'uris': Variant('as', [destination.as_uri()])}]))
            asyncio.get_running_loop().call_later(0.3, response)
            return Message.new_method_return(message, 'o', [request])
        bus.add_message_handler(dispatch)
        await bus.request_name('org.freedesktop.portal.Desktop')
        state_file.with_suffix('.ready').write_text('ready')
        while True:
            await asyncio.sleep(0.08)
            new = int(state_file.read_text())
            if new != scheme[0]:
                scheme[0] = new
                settings.SettingChanged('org.freedesktop.appearance', 'color-scheme', Variant('u', new))
    asyncio.run(run())


class XWindowAttributes(C.Structure):
    # Xlib's public XWindowAttributes layout; Window/Colormap are unsigned long.
    _fields_ = [
        ("x", C.c_int), ("y", C.c_int), ("width", C.c_int),
        ("height", C.c_int), ("border_width", C.c_int), ("depth", C.c_int),
        ("visual", C.c_void_p), ("root", C.c_ulong), ("window_class", C.c_int),
        ("bit_gravity", C.c_int), ("win_gravity", C.c_int),
        ("backing_store", C.c_int), ("backing_planes", C.c_ulong),
        ("backing_pixel", C.c_ulong), ("save_under", C.c_int),
        ("colormap", C.c_ulong), ("map_installed", C.c_int),
        ("map_state", C.c_int), ("all_event_masks", C.c_long),
        ("your_event_mask", C.c_long), ("do_not_propagate_mask", C.c_long),
        ("override_redirect", C.c_int), ("screen", C.c_void_p),
    ]


class X11:
    def __init__(self, display):
        self.x = C.CDLL(ctypes.util.find_library('X11'))
        self.t = C.CDLL(ctypes.util.find_library('Xtst'))
        defs = {
            'XOpenDisplay': ([C.c_char_p], C.c_void_p),
            'XDefaultRootWindow': ([C.c_void_p], C.c_ulong),
            'XQueryTree': ([C.c_void_p,C.c_ulong,C.POINTER(C.c_ulong),C.POINTER(C.c_ulong),C.POINTER(C.POINTER(C.c_ulong)),C.POINTER(C.c_uint)], C.c_int),
            'XFetchName': ([C.c_void_p,C.c_ulong,C.POINTER(C.c_char_p)], C.c_int),
            'XFree': ([C.c_void_p], C.c_int),
            'XSync': ([C.c_void_p,C.c_int], C.c_int),
            'XSetInputFocus': ([C.c_void_p,C.c_ulong,C.c_int,C.c_ulong], C.c_int),
            'XMoveWindow': ([C.c_void_p,C.c_ulong,C.c_int,C.c_int], C.c_int),
            'XResizeWindow': ([C.c_void_p,C.c_ulong,C.c_uint,C.c_uint], C.c_int),
            'XGetGeometry': ([C.c_void_p,C.c_ulong,C.POINTER(C.c_ulong),C.POINTER(C.c_int),C.POINTER(C.c_int),C.POINTER(C.c_uint),C.POINTER(C.c_uint),C.POINTER(C.c_uint),C.POINTER(C.c_uint)],C.c_int),
            'XGetWindowAttributes': ([C.c_void_p,C.c_ulong,C.POINTER(XWindowAttributes)],C.c_int),
            'XStringToKeysym': ([C.c_char_p], C.c_ulong),
            'XKeysymToKeycode': ([C.c_void_p,C.c_ulong],C.c_uint),
            'XCloseDisplay': ([C.c_void_p],C.c_int),
        }
        for name,(args,result) in defs.items():
            fn=getattr(self.x,name);fn.argtypes=args;fn.restype=result
        self.t.XTestFakeMotionEvent.argtypes=[C.c_void_p,C.c_int,C.c_int,C.c_int,C.c_ulong]
        self.t.XTestFakeButtonEvent.argtypes=[C.c_void_p,C.c_uint,C.c_int,C.c_ulong]
        self.t.XTestFakeKeyEvent.argtypes=[C.c_void_p,C.c_uint,C.c_int,C.c_ulong]
        self.d=self.x.XOpenDisplay(display.encode())
        if not self.d: raise RuntimeError('Cannot open Xvfb display')
        self.root=self.x.XDefaultRootWindow(self.d)
    def windows(self):
        root=C.c_ulong();parent=C.c_ulong();kids=C.POINTER(C.c_ulong)();count=C.c_uint()
        self.x.XQueryTree(self.d,self.root,C.byref(root),C.byref(parent),C.byref(kids),C.byref(count))
        result=[]
        for i in range(count.value):
            name=C.c_char_p()
            if self.x.XFetchName(self.d,kids[i],C.byref(name)) and name.value:
                result.append((kids[i],name.value.decode(errors='replace')))
                self.x.XFree(C.cast(name,C.c_void_p))
        if kids: self.x.XFree(kids)
        return result
    def geometry(self,window):
        root=C.c_ulong();x=C.c_int();y=C.c_int();w=C.c_uint();h=C.c_uint();border=C.c_uint();depth=C.c_uint()
        self.x.XGetGeometry(self.d,window,C.byref(root),C.byref(x),C.byref(y),C.byref(w),C.byref(h),C.byref(border),C.byref(depth))
        return x.value,y.value,w.value,h.value
    def is_viewable(self,window):
        attributes = XWindowAttributes()
        return bool(self.x.XGetWindowAttributes(self.d,window,C.byref(attributes))) and attributes.map_state == 2
    def prepare(self,window,width,height):
        # Winit assigns the title before the renderer is ready and the window is
        # mapped. XSetInputFocus on that unmapped window raises fatal BadMatch.
        wait_for(lambda:self.is_viewable(window),35)
        self.x.XMoveWindow(self.d,window,40,40)
        self.x.XResizeWindow(self.d,window,width,height)
        self.x.XSetInputFocus(self.d,window,2,0)
        self.x.XSync(self.d,0)
        time.sleep(0.5)
    def click(self,window,x,y,button=1):
        wx,wy,_,_=self.geometry(window)
        self.t.XTestFakeMotionEvent(self.d,0,wx+int(x),wy+int(y),0)
        self.x.XSync(self.d,0)
        # Let native pointer tracking settle before pressing, especially after
        # opening a modal or resizing the client area on software rendering.
        time.sleep(0.12)
        self.t.XTestFakeButtonEvent(self.d,button,1,0)
        self.x.XSync(self.d,0)
        time.sleep(0.06)
        self.t.XTestFakeButtonEvent(self.d,button,0,0)
        self.x.XSync(self.d,0)
        time.sleep(0.35)


def screenshot(window,path,env):
    subprocess.run(['import','-silent','-window',str(window),str(path)],env=env,check=True,stdout=subprocess.DEVNULL,stderr=subprocess.PIPE)
    from PIL import Image
    image=Image.open(path).convert('RGB')
    return {'path':str(path),'size':list(image.size),'top_left':list(image.getpixel((3,3))),'colors':len(image.getcolors(image.width*image.height) or [])}


def ocr(window,out,env):
    path=out/'_ocr.png';screenshot(window,path,env)
    result=subprocess.run(['tesseract',str(path),'stdout','--psm','11','tsv'],env=env,check=True,capture_output=True,text=True)
    return [row for row in csv.DictReader(io.StringIO(result.stdout),delimiter='\t') if row.get('text','').strip()]


def click_word(x11,window,word,out,env):
    rows=ocr(window,out,env)
    matches=[r for r in rows if r['text'].strip('.,:').casefold()==word.casefold()]
    if not matches: raise RuntimeError(f'Visible button word {word!r} not found: '+ ' '.join(r['text'] for r in rows))
    r=matches[0]
    x11.click(window,int(r['left'])+int(r['width'])/2,int(r['top'])+int(r['height'])/2)


def wait_for(predicate,timeout=20):
    until=time.monotonic()+timeout
    while time.monotonic()<until:
        value=predicate()
        if value: return value
        time.sleep(0.1)
    raise RuntimeError('Timed out waiting for requested QA state')


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('binary',type=Path)
    parser.add_argument('--out',type=Path,default=None)
    parser.add_argument('--inside-bus',action='store_true')
    parser.add_argument('--backend',default=None,help='Optional ICED_BACKEND override for supported renderer verification')
    args=parser.parse_args()
    args.binary=args.binary.resolve(strict=True)
    out=(args.out or Path(tempfile.mkdtemp(prefix='neontrace-ui-qa-'))).resolve()
    out.mkdir(parents=True,exist_ok=True)
    if not args.inside_bus:
        command=['dbus-run-session','--',sys.executable,__file__,str(args.binary),'--out',str(out),'--inside-bus']
        if args.backend: command.extend(['--backend',args.backend])
        raise SystemExit(subprocess.run(command).returncode)
    env=os.environ.copy();env['WINIT_UNIX_BACKEND']='x11';env['WINIT_X11_SCALE_FACTOR']='1';env['LIBGL_ALWAYS_SOFTWARE']='1'
    if args.backend: env['ICED_BACKEND']=args.backend
    display=next(f':{n}' for n in range(91,191) if not Path(f'/tmp/.X11-unix/X{n}').exists())
    env['DISPLAY']=display
    runtime=out/'runtime';runtime.mkdir(mode=0o700,exist_ok=True);env['XDG_RUNTIME_DIR']=str(runtime)
    state=out/'portal-scheme.txt';state.write_text('1')
    logfile=(out/'xvfb.log').open('wb')
    xvfb=subprocess.Popen(['Xvfb',display,'-screen','0','1920x1280x24','-nolisten','tcp'],env=env,stdout=logfile,stderr=logfile)
    portal_log=(out/'portal.log').open('wb')
    portal=subprocess.Popen([sys.executable,__file__,'--portal',str(state),str(out/'exports')],env=env,stdout=portal_log,stderr=portal_log)
    active=[];report={'binary':str(args.binary),'screenshots':[],'checks':{},'errors':[]}
    try:
        wait_for(lambda:Path(f'/tmp/.X11-unix/X{display[1:]}').exists())
        wait_for(lambda:state.with_suffix('.ready').exists())
        x11=X11(display)
        network_guard=offline_network_guard(out)
        def launch(locale,width,height,label,scale="1"):
            folder=out/'profiles'/label
            for var,part in [('XDG_CONFIG_HOME','config'),('XDG_CACHE_HOME','cache'),('XDG_DATA_HOME','data')]:
                (folder/part).mkdir(parents=True,exist_ok=True)
            (folder/'config'/'NeonTrace').mkdir(exist_ok=True)
            (folder/'config'/'NeonTrace'/'settings.json').write_text(json.dumps({'language':locale,'window_width':width,'window_height':height,'history_enabled':False,'tcp_check':False,'reduced_effects':False}))
            localenv=env|{'XDG_CONFIG_HOME':str(folder/'config'),'XDG_CACHE_HOME':str(folder/'cache'),'XDG_DATA_HOME':str(folder/'data'),'WINIT_X11_SCALE_FACTOR':scale,'LD_PRELOAD':str(network_guard),'NEONTRACE_NETWORK_AUDIT':str(out/'network-attempts.txt')}
            log=(out/f'{label}.log').open('wb')
            p=subprocess.Popen([str(args.binary),'--demo'],env=localenv,stdout=log,stderr=log)
            active.append(p)
            def find():
                if p.poll() is not None: raise RuntimeError(f'Native application exited early ({p.returncode}); see {label}.log')
                return next((win for win,title in x11.windows() if title=='NeonTrace'),None)
            window=wait_for(find,35)
            x11.prepare(window,width,height)
            time.sleep(0.8)
            return p,window
        def stop(p):
            p.terminate()
            try:p.wait(5)
            except subprocess.TimeoutExpired:p.kill();p.wait()
            active.remove(p)
            time.sleep(0.1)
        def shot(window,name):
            # A mapped native window can precede the first GPU presentation.
            # Wait for rendered content instead of accepting or failing a black
            # startup frame based on a fixed sleep.
            until=time.monotonic()+10
            while True:
                result=screenshot(window,out/name,env)
                if result['colors']>=30: break
                if time.monotonic()>=until: raise RuntimeError(f'Screenshot looks blank: {name}')
                time.sleep(0.2)
            report['screenshots'].append(result)
            return result
        # Startup dark scheme and live preference change on the same native app.
        p,window=launch('en',1280,800,'english-wide')
        dark=shot(window,'english-1280x800-dark.png')
        state.with_suffix('.next').write_text('2');state.with_suffix('.next').replace(state);time.sleep(0.8)
        light=shot(window,'english-1280x800-live-light.png')
        report['checks']['live_os_theme_changed']=sum(light['top_left'])>sum(dark['top_left'])+100
        if not report['checks']['live_os_theme_changed']: raise RuntimeError('Native window did not visibly follow the portal light-theme change')
        state.with_suffix('.next').write_text('1');state.with_suffix('.next').replace(state);time.sleep(0.4)
        click_word(x11,window,'Settings',out,env);shot(window,'english-settings-1280x800.png')
        click_word(x11,window,'Close',out,env)
        # Real application exports from their dedicated dialog.
        for extension in ['PNG','JSON','CSV']:
            click_word(x11,window,'Export',out,env)
            before=set((out/'exports').glob('*.'+extension.lower()))
            click_word(x11,window,extension,out,env)
            new=wait_for(lambda:next((f for f in (out/'exports').glob('*.'+extension.lower()) if f not in before and f.stat().st_size>0),None),15)
            report['checks']['export_'+extension.lower()]={'path':str(new),'bytes':new.stat().st_size}
        click_word(x11,window,'Multiple',out,env);shot(window,'english-targets-1280x800.png')
        stop(p)
        for locale in ['en','de','hu','fr','it','ru']:
            p,window=launch(locale,680,540,'locale-'+locale)
            item=shot(window,f'locale-{locale}-680x540.png')
            if item['size']!=[680,540]: raise RuntimeError('Unexpected minimum-window screenshot dimensions')
            if locale=='en':
                click_word(x11,window,'Details',out,env);shot(window,'english-details-680x540.png')
                click_word(x11,window,'Close',out,env)
                click_word(x11,window,'Settings',out,env);shot(window,'english-settings-680x540.png')
                click_word(x11,window,'Close',out,env)
                click_word(x11,window,'Targets',out,env);shot(window,'english-targets-680x540.png')
            stop(p)
        p,window=launch('ru',1440,960,'russian-150pct',scale='1.5')
        shot(window,'russian-150pct-1440x960.png');report['checks']['high_dpi_150pct_captured']=True;stop(p)
        # Inspect real serializer outputs independently of UI status text.
        exported_json=Path(report['checks']['export_json']['path'])
        data=json.loads(exported_json.read_text());report['checks']['json_schema_version']=data.get('schema_version')
        report['checks']['json_demonstration']=data.get('demonstration')
        with Path(report['checks']['export_csv']['path']).open(newline='') as f:
            rows=list(csv.reader(f));report['checks']['csv_rows']=len(rows)
        from PIL import Image
        exported_image=Image.open(report['checks']['export_png']['path'])
        report['checks']['png_size']=list(exported_image.size)
        report['checks']['png_license_metadata']=all(key in exported_image.info for key in ['Attribution','Map license','Software license'])
        if data.get('schema_version')!=1 or not data.get('demonstration') or not data.get('sessions'): raise RuntimeError('JSON export schema or demonstration provenance is invalid')
        if len(rows)<20: raise RuntimeError('CSV export omitted measurement rows')
        if not report['checks']['png_license_metadata']: raise RuntimeError('PNG export license metadata is missing')
        report['checks']['six_locales_captured']=True
        report['checks']['offline_no_network_attempts']=not (out/'network-attempts.txt').exists()
        if not report['checks']['offline_no_network_attempts']: raise RuntimeError('Offline demonstration attempted network activity')
    except Exception as exc:
        report['errors'].append(str(exc))
        print('QA ERROR:',str(exc),flush=True)
    finally:
        for p in active:
            p.terminate()
            try:p.wait(3)
            except subprocess.TimeoutExpired:p.kill()
        portal.terminate();xvfb.terminate()
        (out/'report.json').write_text(json.dumps(report,indent=2))
        print(json.dumps({'output':str(out),'checks':report['checks'],'errors':report['errors']},indent=2),flush=True)
    return 1 if report['errors'] else 0


if __name__=='__main__':
    if len(sys.argv)>1 and sys.argv[1]=='--portal':
        portal_main(sys.argv[2],sys.argv[3])
    else:
        raise SystemExit(main())
