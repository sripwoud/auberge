// examples/baikal-busy-blockers.gs
//
// REFERENCE SCRIPT — NOT A SUPPORTED TOOL
// ----------------------------------------
// A worked example of a Busy Feed consumer for operators who want their
// self-hosted Baikal availability to surface to coworkers' "Find a time" on
// their org Google Calendar. auberge produces and serves the Busy Feed
// (sanitized /busy.ics) but ships no consumer; this script runs in the
// operator's own Google account and is theirs to adapt. See ADR-0010.
//
// It fetches the token-gated Busy Feed and upserts opaque "Busy" blocker
// events on the operator's primary calendar, diffing by the feed's hashed
// per-instance UID. No personal event detail is ever read, written, or
// logged — the feed contains none, by design.
//
// Setup (Apps Script editor):
//   1. Project Settings -> Script Properties, add:
//        FEED_URL     https://<baikal-fqdn>/busy.ics
//        FEED_TOKEN   <baikal_busy_feed_token from your auberge config>
//        CALENDAR_ID  primary            (optional; default 'primary')
//        WINDOW_DAYS  60                 (optional; default 60)
//   2. Run syncBusyBlockers once to grant calendar authorization.
//   3. Triggers -> add a time-driven trigger for syncBusyBlockers every 10 min.
//
// Nothing is hardcoded; rotating the token is a Script Properties edit.

var TAG_KEY = 'baikalBusyUid';
var BUSY_TITLE = 'Busy';

function syncBusyBlockers() {
  var props = PropertiesService.getScriptProperties();
  var feedUrl = props.getProperty('FEED_URL');
  var feedToken = props.getProperty('FEED_TOKEN');
  if (!feedUrl || !feedToken) {
    throw new Error('FEED_URL and FEED_TOKEN must be set in Script Properties');
  }
  var calendarId = props.getProperty('CALENDAR_ID') || 'primary';
  var windowDays = parseInt(props.getProperty('WINDOW_DAYS'), 10) || 60;

  var feed = fetchFeed(feedUrl, feedToken);
  var desired = parseFeed(feed);

  var calendar = CalendarApp.getCalendarById(calendarId);
  if (!calendar) {
    throw new Error('Calendar not found: ' + calendarId);
  }

  var now = new Date();
  var windowEnd = new Date(now.getTime() + windowDays * 24 * 60 * 60 * 1000);
  var existing = indexExistingBlockers(calendar, now, windowEnd);

  var created = 0;
  var updated = 0;
  var deleted = 0;
  var seen = {};

  desired.forEach(function (block) {
    seen[block.uid] = true;
    var current = existing[block.uid];
    if (!current) {
      createBlocker(calendar, block);
      created++;
    } else if (timesDiffer(current, block)) {
      applyTimes(current, block);
      updated++;
    }
  });

  Object.keys(existing).forEach(function (uid) {
    if (!seen[uid]) {
      existing[uid].deleteEvent();
      deleted++;
    }
  });

  Logger.log('busy blockers: %s created, %s updated, %s deleted', created, updated, deleted);
}

function fetchFeed(feedUrl, feedToken) {
  var response = UrlFetchApp.fetch(feedUrl, {
    method: 'get',
    headers: { 'X-Auberge-Busy-Token': feedToken },
    muteHttpExceptions: true,
  });
  var code = response.getResponseCode();
  if (code !== 200) {
    throw new Error('Busy Feed fetch failed with HTTP ' + code);
  }
  return response.getContentText();
}

function parseFeed(text) {
  var blocks = text.split('BEGIN:VEVENT').slice(1);
  var events = [];
  blocks.forEach(function (block) {
    var uid = firstMatch(block, /\r?\nUID:([^\r\n]+)/);
    var start = parseIcsDate(matchDate(block, 'DTSTART'));
    var end = parseIcsDate(matchDate(block, 'DTEND'));
    if (uid && start && end) {
      events.push({ uid: uid, start: start.date, end: end.date, allDay: start.allDay });
    }
  });
  return events;
}

function matchDate(block, field) {
  var match = block.match(new RegExp('\\r?\\n' + field + '(;VALUE=DATE)?:([0-9TZ]+)'));
  return match ? match[2] : null;
}

function firstMatch(text, regex) {
  var match = text.match(regex);
  return match ? match[1].trim() : null;
}

function parseIcsDate(value) {
  if (!value) {
    return null;
  }
  var year = parseInt(value.substr(0, 4), 10);
  var month = parseInt(value.substr(4, 2), 10) - 1;
  var day = parseInt(value.substr(6, 2), 10);
  if (value.length === 8) {
    return { allDay: true, date: new Date(year, month, day) };
  }
  var hour = parseInt(value.substr(9, 2), 10);
  var minute = parseInt(value.substr(11, 2), 10);
  var second = parseInt(value.substr(13, 2), 10);
  return { allDay: false, date: new Date(Date.UTC(year, month, day, hour, minute, second)) };
}

function indexExistingBlockers(calendar, start, end) {
  var index = {};
  calendar.getEvents(start, end).forEach(function (event) {
    var uid = event.getTag(TAG_KEY);
    if (uid) {
      index[uid] = event;
    }
  });
  return index;
}

function createBlocker(calendar, block) {
  var event = block.allDay
    ? calendar.createAllDayEvent(BUSY_TITLE, block.start, block.end)
    : calendar.createEvent(BUSY_TITLE, block.start, block.end);
  event.setVisibility(CalendarApp.Visibility.PRIVATE);
  event.setTag(TAG_KEY, block.uid);
}

function timesDiffer(event, block) {
  return (
    event.getStartTime().getTime() !== block.start.getTime() ||
    event.getEndTime().getTime() !== block.end.getTime()
  );
}

function applyTimes(event, block) {
  if (block.allDay) {
    event.setAllDayDates(block.start, block.end);
  } else {
    event.setTime(block.start, block.end);
  }
}
