//! Complete, compile-time checked UI vocabulary. Provider strings, addresses,
//! hostnames and measured values are deliberately never translated.
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    En,
    De,
    Hu,
    Fr,
    It,
    Ru,
}

impl Language {
    pub const ALL: [Self; 6] = [Self::En, Self::De, Self::Hu, Self::Fr, Self::It, Self::Ru];
    pub const fn index(self) -> usize {
        match self {
            Self::En => 0,
            Self::De => 1,
            Self::Hu => 2,
            Self::Fr => 3,
            Self::It => 4,
            Self::Ru => 5,
        }
    }
    pub const fn code(self) -> &'static str {
        ["en", "de", "hu", "fr", "it", "ru"][self.index()]
    }
    pub const fn name(self) -> &'static str {
        [
            "English",
            "Deutsch",
            "Magyar",
            "Français",
            "Italiano",
            "Русский",
        ][self.index()]
    }
    pub fn from_code(code: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|v| v.code() == code)
            .unwrap_or_default()
    }
    pub const fn text(self, key: Key) -> &'static str {
        key.translations()[self.index()]
    }
    /// Replace named placeholders. UI code supplies already formatted values.
    /// This avoids positional argument order assumptions between languages.
    pub fn format(self, key: Key, values: &[(&str, &str)]) -> String {
        let mut out = self.text(key).to_owned();
        for (name, value) in values {
            out = out.replace(&format!("{{{name}}}"), value);
        }
        out
    }
}
impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

macro_rules! vocabulary {
    ($($key:ident => [$en:literal, $de:literal, $hu:literal, $fr:literal, $it:literal, $ru:literal]),* $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Key { $($key),* }
        impl Key {
            #[cfg(test)]
            pub const ALL: &'static [Key] = &[$(Self::$key),*];
            pub const fn translations(self) -> [&'static str; 6] {
                match self { $(Self::$key => [$en, $de, $hu, $fr, $it, $ru]),* }
            }
        }
    }
}

vocabulary! {
    ClearView => ["Clear", "Leeren", "Törlés", "Effacer", "Svuota", "Очистить"],
    FitMap => ["Fit route", "Route einpassen", "Útvonalra igazítás", "Ajuster au parcours", "Adatta al percorso", "Вписать маршрут"],
    MapNavigationHint => ["Ctrl + scroll to zoom · Ctrl + drag to move", "Strg + Mausrad zum Zoomen · Strg + Ziehen zum Verschieben", "Ctrl + görgetés: nagyítás · Ctrl + húzás: mozgatás", "Ctrl + défilement : zoom · Ctrl + glisser : déplacement", "Ctrl + rotella per lo zoom · Ctrl + trascina per spostare", "Ctrl + прокрутка — масштаб · Ctrl + перетаскивание — перемещение"],
    MultiViewHint => ["Animated routes · select a target for details", "Animierte Routen · Ziel für Details auswählen", "Animált útvonalak · részletekhez válasszon célpontot", "Parcours animés · sélectionnez une cible pour les détails", "Percorsi animati · seleziona una destinazione per i dettagli", "Анимация маршрутов · выберите цель для подробностей"],
    Licenses => ["Licenses", "Lizenzen", "Licencek", "Licences", "Licenze", "Лицензии"],
    AboutIntro => ["A native visual ping and route explorer.", "Ein natives Programm für visuelle Pings und Netzwerkrouten.", "Natív alkalmazás a ping és a hálózati útvonalak megjelenítésére.", "Une application native pour visualiser le ping et les parcours réseau.", "Un’applicazione nativa per visualizzare ping e percorsi di rete.", "Нативное приложение для визуального пинга и сетевых маршрутов."],
    AboutDescription => ["Follow your connections on the map, inspect observed hops and compare measured round-trip times.", "Verbindungen auf der Karte verfolgen, beobachtete Hops untersuchen und gemessene Umlaufzeiten vergleichen.", "Kövesse a kapcsolatait a térképen, vizsgálja meg a megfigyelt ugrásokat, és hasonlítsa össze a mért oda-vissza időket.", "Suivez vos connexions sur la carte, examinez les sauts observés et comparez les temps aller-retour mesurés.", "Segui le connessioni sulla mappa, esamina gli hop osservati e confronta i tempi di andata e ritorno misurati.", "Следите за соединениями на карте, изучайте обнаруженные узлы и сравнивайте измеренное время отклика."],
    ExportHint => ["Save the current view or export your observations.", "Aktuelle Ansicht speichern oder Beobachtungen exportieren.", "Mentse az aktuális nézetet, vagy exportálja a megfigyeléseket.", "Enregistrez la vue actuelle ou exportez vos observations.", "Salva la vista attuale o esporta le osservazioni.", "Сохраните текущий вид или экспортируйте наблюдения."],
    ExportPngHint => ["An image of the current application view.", "Ein Bild der aktuellen Anwendungsansicht.", "Kép az alkalmazás aktuális nézetéről.", "Une image de la vue actuelle de l’application.", "Un’immagine della vista attuale dell’applicazione.", "Изображение текущего окна приложения."],
    ExportJsonHint => ["Complete observations and route snapshots for reuse.", "Vollständige Beobachtungen und Routenaufnahmen zur weiteren Verwendung.", "Teljes megfigyelések és útvonalpillanatképek további felhasználásra.", "Observations complètes et instantanés des parcours pour réutilisation.", "Osservazioni complete e istantanee dei percorsi da riutilizzare.", "Полные наблюдения и снимки маршрутов для дальнейшего использования."],
    ExportCsvHint => ["Samples and hops in a spreadsheet-friendly table.", "Messwerte und Hops als Tabelle für Tabellenkalkulationen.", "Mérési minták és ugrások táblázatkezelőben használható formában.", "Échantillons et sauts dans un tableau adapté aux tableurs.", "Campioni e hop in una tabella adatta ai fogli di calcolo.", "Измерения и узлы в таблице для табличных редакторов."],
    ExportPrivacy => ["Exported files may include target addresses and observations.", "Exportierte Dateien können Zieladressen und Beobachtungen enthalten.", "Az exportált fájlok célcímeket és megfigyeléseket is tartalmazhatnak.", "Les fichiers exportés peuvent contenir les adresses des cibles et les observations.", "I file esportati possono includere indirizzi delle destinazioni e osservazioni.", "Экспортированные файлы могут содержать адреса целей и наблюдения."],
    SettingsHint => ["Preferences are saved on this device.", "Einstellungen werden auf diesem Gerät gespeichert.", "A beállításokat ezen az eszközön mentjük.", "Les préférences sont enregistrées sur cet appareil.", "Le preferenze vengono salvate su questo dispositivo.", "Настройки сохраняются на этом устройстве."],
    AppSubtitle => ["Visual ping + route explorer", "Ping und Routen visualisieren", "Vizuális ping és útvonalfelderítés", "Ping visuel et exploration de routes", "Ping visivo ed esplorazione percorsi", "Визуальный пинг и маршруты"],
    WorldConnected => ["THE WORLD, CONNECTED", "DIE WELT, VERNETZT", "A VILÁG ÖSSZEKÖTVE", "LE MONDE, CONNECTÉ", "IL MONDO, CONNESSO", "МИР НА СВЯЗИ"],
    Overview => ["Overview", "Übersicht", "Áttekintés", "Vue d’ensemble", "Panoramica", "Обзор"],
    MapFocus => ["Map focus", "Kartenfokus", "Térképnézet", "Centrer sur la carte", "Solo mappa", "Только карта"],
    ShowPanel => ["Show panel", "Panel anzeigen", "Panel megjelenítése", "Afficher le panneau", "Mostra pannello", "Показать панель"],
    Journey => ["The journey", "Die Reise", "Az útvonal", "Le parcours", "Il percorso", "Путь"],
    FromThisComputer => ["From this computer", "Von diesem Computer", "Erről a számítógépről", "Depuis cet ordinateur", "Da questo computer", "С этого компьютера"],
    SelectedHop => ["Selected location", "Ausgewählter Ort", "Kiválasztott hely", "Lieu sélectionné", "Posizione selezionata", "Выбранная точка"],
    MoreDetails => ["More details", "Weitere Details", "További részletek", "Plus de détails", "Altri dettagli", "Подробнее"],
    HideDetails => ["Hide details", "Details ausblenden", "Részletek elrejtése", "Masquer les détails", "Nascondi dettagli", "Скрыть подробности"],
    EnterDestination => ["Enter a destination to explore its route", "Ziel eingeben, um die Route zu erkunden", "Adjon meg egy célpontot az útvonal feltérképezéséhez", "Saisissez une destination pour explorer son parcours", "Inserisci una destinazione per esplorarne il percorso", "Введите цель для исследования маршрута"],
    ExampleRoute => ["Example route", "Beispielroute", "Példaútvonal", "Parcours d’exemple", "Percorso di esempio", "Пример маршрута"],
    LocatedRoute => ["Located route", "Verortete Route", "Térképre helyezett útvonal", "Parcours localisé", "Percorso localizzato", "Маршрут на карте"],
    UnknownSection => ["Location unknown", "Ort unbekannt", "Ismeretlen hely", "Position inconnue", "Posizione sconosciuta", "Местоположение неизвестно"],
    DisplayedJourney => ["8-second visual journey", "8 Sekunden Routenanimation", "8 másodperces útvonalanimáció", "Parcours visuel de 8 secondes", "Percorso visivo di 8 secondi", "Анимация маршрута: 8 секунд"],
    DemoShort => ["Offline demo", "Offline-Demo", "Offline bemutató", "Démo hors ligne", "Demo offline", "Автономное демо"],
    SelectLocation => ["Select a location on the map", "Ort auf der Karte auswählen", "Válasszon egy helyet a térképen", "Sélectionnez un lieu sur la carte", "Seleziona una posizione sulla mappa", "Выберите точку на карте"],
    Targets => ["Targets", "Ziele", "Célpontok", "Cibles", "Destinazioni", "Цели"],
    AllHops => ["All observed hops", "Alle beobachteten Hops", "Összes megfigyelt ugrás", "Tous les sauts observés", "Tutti gli hop osservati", "Все обнаруженные узлы"],
    NoHops => ["No route observations yet", "Noch keine Routenbeobachtungen", "Még nincs útvonalmérési adat", "Aucune observation du parcours", "Nessuna osservazione del percorso", "Данных о маршруте пока нет"],
    Target => ["Target", "Ziel", "Célpont", "Cible", "Destinazione", "Цель"],
    TargetHint => ["Hostname, IP address or URL", "Hostname, IP-Adresse oder URL", "Gépnév, IP-cím vagy URL", "Nom d’hôte, adresse IP ou URL", "Nome host, indirizzo IP o URL", "Имя узла, IP-адрес или URL"],
    Start => ["Start", "Starten", "Indítás", "Démarrer", "Avvia", "Начать"],
    Stop => ["Stop", "Stoppen", "Leállítás", "Arrêter", "Ferma", "Остановить"],
    TraceAgain => ["Trace again", "Route erneut ermitteln", "Útvonal újramérése", "Relancer le traçage", "Ripeti tracciamento", "Повторить трассировку"],
    Replay => ["Replay", "Erneut abspielen", "Újrajátszás", "Rejouer", "Riproduci", "Повторить показ"],
    Pause => ["Pause animation", "Animation pausieren", "Animáció szüneteltetése", "Suspendre l’animation", "Pausa animazione", "Приостановить анимацию"],
    Resume => ["Resume animation", "Animation fortsetzen", "Animáció folytatása", "Reprendre l’animation", "Riprendi animazione", "Продолжить анимацию"],
    SingleTarget => ["Single target", "Einzelziel", "Egy célpont", "Cible unique", "Destinazione singola", "Одна цель"],
    MultiTarget => ["Multiple targets", "Mehrere Ziele", "Több célpont", "Cibles multiples", "Più destinazioni", "Несколько целей"],
    ConfigureTargets => ["Configure targets", "Ziele konfigurieren", "Célpontok beállítása", "Configurer les cibles", "Configura destinazioni", "Настроить цели"],
    TargetListHint => ["Configure 1–10 targets. Each target runs independently.", "1–10 Ziele konfigurieren. Jedes Ziel wird unabhängig geprüft.", "Adjon meg 1–10 célpontot. A mérések egymástól függetlenek.", "Configurez 1 à 10 cibles. Chaque cible est mesurée séparément.", "Configura da 1 a 10 destinazioni. Ognuna viene misurata separatamente.", "Укажите от 1 до 10 целей. Измерения выполняются независимо."],
    AddTarget => ["Add target", "Ziel hinzufügen", "Célpont hozzáadása", "Ajouter une cible", "Aggiungi destinazione", "Добавить цель"],
    Remove => ["Remove", "Entfernen", "Eltávolítás", "Retirer", "Rimuovi", "Удалить"],
    Apply => ["Apply", "Übernehmen", "Alkalmazás", "Appliquer", "Applica", "Применить"],
    Cancel => ["Cancel", "Abbrechen", "Mégse", "Annuler", "Annulla", "Отмена"],
    Close => ["Close", "Schließen", "Bezárás", "Fermer", "Chiudi", "Закрыть"],
    Settings => ["Settings", "Einstellungen", "Beállítások", "Paramètres", "Impostazioni", "Настройки"],
    Language => ["Language", "Sprache", "Nyelv", "Langue", "Lingua", "Язык"],
    About => ["About", "Über NeonTrace", "Névjegy", "À propos", "Informazioni", "О программе"],
    DemoWarning => ["Fictional observations · no network probes", "Fiktive Beobachtungen · keine Netzwerkproben", "Kitalált megfigyelések · nincs hálózati mérés", "Observations fictives · aucune sonde réseau", "Osservazioni fittizie · nessuna sonda di rete", "Вымышленные наблюдения · без сетевых запросов"],
    DemoLabel => ["DEMO", "DEMO", "BEMUTATÓ", "DÉMO", "DEMO", "ДЕМО"],
    ObservedRoute => ["Observed probe route · approximate locations", "Beobachtete Proberoute · ungefähre Orte", "Megfigyelt mérési útvonal · becsült helyek", "Itinéraire observé · positions approximatives", "Percorso osservato · posizioni approssimative", "Наблюдаемый маршрут · примерные местоположения"],
    LocationUnknown => ["Location unknown", "Ort unbekannt", "Ismeretlen hely", "Position inconnue", "Posizione sconosciuta", "Местоположение неизвестно"],
    Unknown => ["Unknown", "Unbekannt", "Ismeretlen", "Inconnu", "Sconosciuto", "Неизвестно"],
    Endpoint => ["Endpoint", "Endpunkt", "Végpont", "Point de terminaison", "Endpoint", "Конечный узел"],
    Origin => ["Public egress", "Öffentlicher Netzausgang", "Nyilvános hálózati kijárat", "Sortie réseau publique", "Uscita di rete pubblica", "Публичный сетевой выход"],
    EstimatedOrigin => ["Estimated network egress (ISP/VPN), not the physical location of your device.", "Geschätzter Netzausgang (Internetanbieter/VPN), nicht der physische Standort Ihres Geräts.", "Becsült hálózati kijárat (internetszolgáltató/VPN), nem az eszköz fizikai helye.", "Sortie réseau estimée (FAI/VPN), pas la position physique de votre appareil.", "Uscita di rete stimata (provider/VPN), non la posizione fisica del dispositivo.", "Примерное местоположение сетевого выхода (провайдер/VPN), а не самого устройства."],
    YourPc => ["Your PC", "Ihr PC", "Saját számítógép", "Votre PC", "Il tuo PC", "Ваш компьютер"],
    CurrentRtt => ["Current RTT", "Aktuelle RTT", "Aktuális RTT", "RTT actuel", "RTT attuale", "Текущее RTT"],
    Minimum => ["Minimum", "Minimum", "Minimum", "Minimum", "Minimo", "Минимум"],
    Average => ["Average", "Durchschnitt", "Átlag", "Moyenne", "Media", "Среднее"],
    Maximum => ["Maximum", "Maximum", "Maximum", "Maximum", "Massimo", "Максимум"],
    Replies => ["Replies", "Antworten", "Válaszok", "Réponses", "Risposte", "Ответы"],
    Nonresponses => ["Nonresponses", "Ohne Antwort", "Válasz nélkül", "Sans réponse", "Senza risposta", "Без ответа"],
    SampleWindow => ["Statistics cover the latest 3,600 endpoint probes.", "Statistiken umfassen die letzten 3.600 Zielproben.", "A statisztikák a legutóbbi 3600 végpontmérést fedik le.", "Les statistiques couvrent les 3 600 dernières sondes de la cible.", "Le statistiche coprono le ultime 3.600 sonde della destinazione.", "Статистика охватывает последние 3600 проверок конечного узла."],
    RouteSnapshot => ["Route snapshot", "Routenaufnahme", "Útvonalpillanatkép", "Instantané du tracé", "Istantanea del percorso", "Снимок маршрута"],
    Hop => ["Hop", "Hop", "Ugrás", "Saut", "Hop", "Узел"],
    Hops => ["Hops", "Hops", "Ugrások", "Sauts", "Hop", "Узлы"],
    TcpCheck => ["Separate TCP 443 check", "Separate TCP-443-Prüfung", "Külön TCP 443 ellenőrzés", "Test TCP 443 distinct", "Verifica TCP 443 separata", "Отдельная проверка TCP 443"],
    TcpNote => ["Connection timing only; not ICMP or website health. Applies on Start.", "Nur Verbindungsdauer; kein ICMP- oder Website-Test. Gilt ab dem nächsten Start.", "Csak kapcsolódási idő; nem ICMP- vagy webhelyállapot-mérés. Indításkor lép életbe.", "Durée de connexion uniquement ; ni ICMP ni test de santé du site. Appliqué au démarrage.", "Solo tempo di connessione; non è ICMP né un test del sito. Si applica all’avvio.", "Только время соединения; это не ICMP и не проверка сайта. Применяется при запуске измерений."],
    Idle => ["Ready · measurements stopped", "Bereit · Messungen gestoppt", "Kész · mérések leállítva", "Prêt · mesures arrêtées", "Pronto · misurazioni ferme", "Готово · измерения остановлены"],
    Resolving => ["Resolving target…", "Ziel wird aufgelöst…", "Célpont feloldása…", "Résolution de la cible…", "Risoluzione della destinazione…", "Определение адреса цели…"],
    Measuring => ["Measuring", "Messung läuft", "Mérés folyamatban", "Mesure en cours", "Misurazione in corso", "Измерение"],
    Tracing => ["Discovering route…", "Route wird ermittelt…", "Útvonal felderítése…", "Découverte du tracé…", "Rilevamento del percorso…", "Определение маршрута…"],
    PartialRoute => ["Partial route", "Unvollständige Route", "Részleges útvonal", "Tracé partiel", "Percorso parziale", "Неполный маршрут"],
    NetworkChanged => ["Network changed · measurements stopped; start a fresh session.", "Netzwerk geändert · Messungen gestoppt; neue Sitzung starten.", "A hálózat megváltozott · a mérés leállt; indítson új munkamenetet.", "Réseau modifié · mesures arrêtées ; démarrez une nouvelle session.", "Rete cambiata · misurazioni ferme; avvia una nuova sessione.", "Сеть изменилась · измерения остановлены; начните новый сеанс."],
    Stopped => ["Stopped", "Gestoppt", "Leállítva", "Arrêté", "Fermato", "Остановлено"],
    NoReply => ["No reply", "Keine Antwort", "Nincs válasz", "Aucune réponse", "Nessuna risposta", "Нет ответа"],
    NoRoute => ["No route snapshot available", "Keine Routenaufnahme verfügbar", "Nincs útvonalpillanatkép", "Aucun instantané du tracé", "Nessuna istantanea disponibile", "Снимок маршрута отсутствует"],
    PermissionDenied => ["ICMP probe permission unavailable. The separate TCP 443 check remains available.", "Keine Berechtigung für ICMP-Proben. Die separate TCP-443-Prüfung bleibt verfügbar.", "Nincs jogosultság ICMP-méréshez. A külön TCP 443 ellenőrzés továbbra is elérhető.", "Autorisation ICMP indisponible. Le test TCP 443 distinct reste disponible.", "Permesso ICMP non disponibile. La verifica TCP 443 separata resta disponibile.", "Недостаточно прав для ICMP. Отдельная проверка TCP 443 остаётся доступной."],
    TraceUnavailable => ["Tracing unavailable; endpoint measurements remain separate.", "Routenermittlung nicht verfügbar; Endpunktmessungen bleiben getrennt.", "Az útvonalmérés nem érhető el; a végpontmérés ettől független.", "Traçage indisponible ; les mesures de la cible restent distinctes.", "Tracciamento non disponibile; le misure dell’endpoint restano separate.", "Трассировка недоступна; измерения конечного узла выполняются отдельно."],
    InvalidTarget => ["Enter a valid hostname, IPv4/IPv6 address or URL.", "Gültigen Hostnamen, IPv4-/IPv6-Adresse oder URL eingeben.", "Érvényes gépnevet, IPv4-/IPv6-címet vagy URL-t adjon meg.", "Saisissez un nom d’hôte, une adresse IPv4/IPv6 ou une URL valide.", "Inserisci un nome host, indirizzo IPv4/IPv6 o URL valido.", "Введите допустимое имя узла, адрес IPv4/IPv6 или URL."],
    ResolutionFailed => ["Target resolution failed.", "Zielauflösung fehlgeschlagen.", "A célpont feloldása sikertelen.", "Échec de la résolution de la cible.", "Risoluzione della destinazione non riuscita.", "Не удалось определить адрес цели."],
    NetworkError => ["Network operation failed.", "Netzwerkvorgang fehlgeschlagen.", "A hálózati művelet sikertelen.", "Échec de l’opération réseau.", "Operazione di rete non riuscita.", "Ошибка сетевой операции."],
    Appearance => ["Appearance", "Darstellung", "Megjelenés", "Apparence", "Aspetto", "Оформление"],
    SystemTheme => ["Colours follow the operating system.", "Farben folgen dem Betriebssystem.", "A színek az operációs rendszert követik.", "Les couleurs suivent le système d’exploitation.", "I colori seguono il sistema operativo.", "Цвета определяются операционной системой."],
    ReducedEffects => ["Reduced rendering effects", "Reduzierte Darstellungseffekte", "Csökkentett megjelenítési effektek", "Effets visuels réduits", "Effetti grafici ridotti", "Упрощённые графические эффекты"],
    History => ["History", "Verlauf", "Előzmények", "Historique", "Cronologia", "История"],
    EnableHistory => ["Save local history", "Lokalen Verlauf speichern", "Helyi előzmények mentése", "Enregistrer l’historique local", "Salva cronologia locale", "Сохранять локальную историю"],
    HistoryDisabled => ["History is disabled. Targets remain in memory only.", "Verlauf deaktiviert. Ziele bleiben nur im Arbeitsspeicher.", "Az előzmények ki vannak kapcsolva. A célpontok csak a memóriában maradnak.", "Historique désactivé. Les cibles restent uniquement en mémoire.", "Cronologia disattivata. Le destinazioni rimangono solo in memoria.", "История отключена. Цели хранятся только в памяти."],
    ClearHistory => ["Clear history", "Verlauf löschen", "Előzmények törlése", "Effacer l’historique", "Cancella cronologia", "Очистить историю"],
    DeleteRecord => ["Delete record", "Eintrag löschen", "Bejegyzés törlése", "Supprimer l’enregistrement", "Elimina voce", "Удалить запись"],
    EmptyHistory => ["No saved observations", "Keine gespeicherten Beobachtungen", "Nincs mentett megfigyelés", "Aucune observation enregistrée", "Nessuna osservazione salvata", "Сохранённых наблюдений нет"],
    SavedResult => ["Saved observation · not live", "Gespeicherte Beobachtung · nicht live", "Mentett megfigyelés · nem élő", "Observation enregistrée · hors direct", "Osservazione salvata · non in tempo reale", "Сохранённое наблюдение · не текущее"],
    HistoryRetention => ["The newest 100 sessions are retained.", "Die neuesten 100 Sitzungen werden gespeichert.", "A legutóbbi 100 munkamenetet őrizzük meg.", "Les 100 sessions les plus récentes sont conservées.", "Vengono conservate le 100 sessioni più recenti.", "Сохраняются 100 последних сеансов."],
    StorageError => ["Could not save local data.", "Lokale Daten konnten nicht gespeichert werden.", "A helyi adatok mentése sikertelen.", "Impossible d’enregistrer les données locales.", "Impossibile salvare i dati locali.", "Не удалось сохранить локальные данные."],
    Export => ["Export", "Exportieren", "Exportálás", "Exporter", "Esporta", "Экспорт"],
    ExportPng => ["Export PNG", "PNG exportieren", "PNG exportálása", "Exporter en PNG", "Esporta PNG", "Экспорт PNG"],
    ExportJson => ["Export JSON", "JSON exportieren", "JSON exportálása", "Exporter en JSON", "Esporta JSON", "Экспорт JSON"],
    ExportCsv => ["Export CSV", "CSV exportieren", "CSV exportálása", "Exporter en CSV", "Esporta CSV", "Экспорт CSV"],
    ExportSucceeded => ["Export saved", "Export gespeichert", "Export mentve", "Export enregistré", "Esportazione salvata", "Экспорт сохранён"],
    ExportFailed => ["Export failed.", "Export fehlgeschlagen.", "Sikertelen exportálás.", "Échec de l’export.", "Esportazione non riuscita.", "Не удалось экспортировать."],
    NothingToExport => ["No observations to export.", "Keine Beobachtungen zum Exportieren.", "Nincs exportálható megfigyelés.", "Aucune observation à exporter.", "Nessuna osservazione da esportare.", "Нет наблюдений для экспорта."],
    Measurements => ["Measurements", "Messungen", "Mérések", "Mesures", "Misurazioni", "Измерения"],
    DataSource => ["Data source", "Datenquelle", "Adatforrás", "Source des données", "Fonte dei dati", "Источник данных"],
    DataEdition => ["Database edition", "Datenbankausgabe", "Adatbázis-kiadás", "Édition de la base", "Edizione del database", "Версия базы данных"],
    LocationData => ["Location databases", "Ortsdatenbanken", "Helyadatbázisok", "Bases de localisation", "Database di geolocalizzazione", "Базы геолокации"],
    DownloadOnFirstUse => ["City and ASN data download at first live use.", "Stadt- und ASN-Daten werden bei der ersten Live-Nutzung geladen.", "A város- és ASN-adatok az első élő használatkor töltődnek le.", "Les données de ville et d’ASN sont téléchargées à la première mesure réelle.", "I dati città e ASN vengono scaricati al primo utilizzo reale.", "Данные городов и ASN загружаются при первом реальном измерении."],
    CityDatabase => ["City", "Städte", "Város", "Villes", "Città", "Города"],
    AsnDatabase => ["ASN", "ASN", "ASN", "ASN", "ASN", "ASN"],
    DownloadingCity => ["Downloading City database…", "Stadtdatenbank wird geladen…", "Városadatbázis letöltése…", "Téléchargement de la base des villes…", "Download del database città…", "Загрузка базы городов…"],
    DownloadingAsn => ["Downloading ASN database…", "ASN-Datenbank wird geladen…", "ASN-adatbázis letöltése…", "Téléchargement de la base ASN…", "Download del database ASN…", "Загрузка базы ASN…"],
    ValidatingDatabase => ["Validating downloaded database…", "Geladene Datenbank wird geprüft…", "Letöltött adatbázis ellenőrzése…", "Vérification de la base téléchargée…", "Verifica del database scaricato…", "Проверка загруженной базы…"],
    DatabaseReady => ["Cached databases ready", "Zwischengespeicherte Datenbanken bereit", "Gyorsítótárazott adatbázisok készen", "Bases en cache prêtes", "Database in cache pronti", "Сохранённые базы готовы"],
    DatabaseMissing => ["Location data unavailable; probes can continue.", "Ortsdaten fehlen; Proben können fortgesetzt werden.", "Nincs helyadat; a mérések folytathatók.", "Localisation indisponible ; les sondes peuvent continuer.", "Dati posizione non disponibili; le sonde possono continuare.", "Данные местоположений недоступны; измерения могут продолжаться."],
    DatabaseFailed => ["Database update failed; the last valid cache is retained.", "Datenbankaktualisierung fehlgeschlagen; letzter gültiger Cache bleibt erhalten.", "Az adatbázis-frissítés sikertelen; az utolsó érvényes példány megmarad.", "Échec de mise à jour ; le dernier cache valide est conservé.", "Aggiornamento non riuscito; l’ultima cache valida viene conservata.", "Обновление базы не удалось; последняя исправная копия сохранена."],
    OriginUnavailable => ["Public egress unavailable; no origin is invented.", "Öffentlicher Netzausgang unbekannt; kein Ursprung wird erfunden.", "A nyilvános kijárat ismeretlen; nem adunk meg kitalált kiindulópontot.", "Sortie publique indisponible ; aucune origine n’est inventée.", "Uscita pubblica non disponibile; non viene inventata un’origine.", "Публичный выход недоступен; начальная точка не выдумывается."],
    Refresh => ["Refresh databases", "Datenbanken aktualisieren", "Adatbázisok frissítése", "Actualiser les bases", "Aggiorna database", "Обновить базы"],
    CheckingUpdates => ["Checking monthly database editions…", "Monatliche Datenbankausgaben werden geprüft…", "Havi adatbázis-kiadások ellenőrzése…", "Vérification des éditions mensuelles…", "Verifica delle edizioni mensili…", "Проверка ежемесячных версий баз…"],
    Privacy => ["Privacy", "Datenschutz", "Adatvédelem", "Confidentialité", "Privacy", "Конфиденциальность"],
    PrivacyNote => ["No telemetry. Live mode uses DNS, probes, ipify and DB-IP downloads. Hop lookups stay local.", "Keine Telemetrie. Live-Modus nutzt DNS, Proben, ipify und DB-IP-Downloads. Hop-Abfragen bleiben lokal.", "Nincs telemetria. Az élő mód DNS-t, méréseket, ipify-t és DB-IP-letöltéseket használ. Az ugrások keresése helyben történik.", "Aucune télémétrie. Le mode réel utilise DNS, sondes, ipify et téléchargements DB-IP. Les recherches de sauts restent locales.", "Nessuna telemetria. La modalità reale usa DNS, sonde, ipify e download DB-IP. Le ricerche degli hop restano locali.", "Без телеметрии. Реальный режим использует DNS, зонды, ipify и загрузки DB-IP. Поиск узлов выполняется локально."],
    Credits => ["Licenses and data credits", "Lizenzen und Datenquellen", "Licencek és adatforrások", "Licences et sources", "Licenze e fonti dei dati", "Лицензии и источники данных"],
    MapCredit => ["Map: Simplemaps · Robinson projection · Antarctica not included", "Karte: Simplemaps · Robinson-Projektion · ohne Antarktis", "Térkép: Simplemaps · Robinson-vetület · Antarktisz nélkül", "Carte : Simplemaps · projection Robinson · sans Antarctique", "Mappa: Simplemaps · proiezione Robinson · Antartide esclusa", "Карта: Simplemaps · проекция Робинсона · без Антарктиды"],
    GeoCredit => ["IP Geolocation by DB-IP · CC BY 4.0", "IP-Geolokalisierung von DB-IP · CC BY 4.0", "IP-helymeghatározás: DB-IP · CC BY 4.0", "Géolocalisation IP par DB-IP · CC BY 4.0", "Geolocalizzazione IP di DB-IP · CC BY 4.0", "IP-геолокация: DB-IP · CC BY 4.0"],
    SecondsValue => ["{value} s", "{value} s", "{value} mp", "{value} s", "{value} s", "{value} с"],
    Complete => ["Complete", "Abgeschlossen", "Kész", "Terminé", "Completato", "Завершено"],
    Unavailable => ["Unavailable", "Nicht verfügbar", "Nem érhető el", "Indisponible", "Non disponibile", "Недоступно"],
    Details => ["Details", "Details", "Részletek", "Détails", "Dettagli", "Подробности"],
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    fn placeholders(s: &str) -> BTreeSet<&str> {
        s.split('{')
            .skip(1)
            .filter_map(|rest| rest.split_once('}').map(|(name, _)| name))
            .collect()
    }
    #[test]
    fn every_locale_has_every_key_and_matching_parameters() {
        for &key in Key::ALL {
            let translations = key.translations();
            let expected = placeholders(translations[0]);
            for lang in Language::ALL {
                let value = lang.text(key);
                assert!(!value.trim().is_empty(), "{lang:?}/{key:?} is empty");
                assert_eq!(
                    placeholders(value),
                    expected,
                    "{lang:?}/{key:?}: parameters differ"
                );
                assert!(
                    !value.contains('\u{fffd}'),
                    "{lang:?}/{key:?}: replacement glyph"
                );
            }
        }
    }
    #[test]
    fn english_is_default_and_only_six_languages_exist() {
        assert_eq!(Language::default(), Language::En);
        assert_eq!(Language::ALL.len(), 6);
        for language in Language::ALL {
            assert_eq!(Language::from_code(language.code()), language);
        }
    }
    #[test]
    fn named_parameters_use_localized_units() {
        assert_eq!(
            Language::Hu.format(Key::SecondsValue, &[("value", "3")]),
            "3 mp"
        );
        assert_eq!(
            Language::Fr.format(Key::SecondsValue, &[("value", "3")]),
            "3 s"
        );
    }
}
