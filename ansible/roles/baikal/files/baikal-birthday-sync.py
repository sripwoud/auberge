#!/usr/bin/env python3
"""
Baikal Birthday Calendar Sync Script

Reads contacts from Baikal's SQLite database, extracts birthday information,
and creates/updates a birthday calendar with recurring annual events.
"""

import sqlite3
import sys
from datetime import datetime
import re
import hashlib


class BaikalBirthdaySync:
    def __init__(self, db_path):
        self.db_path = db_path
        self.conn = None
        self.birthday_calendar_id = None

    def connect(self):
        """Connect to the Baikal SQLite database."""
        try:
            self.conn = sqlite3.connect(self.db_path)
            self.conn.row_factory = sqlite3.Row
            return True
        except sqlite3.Error as e:
            print(f"Error connecting to database: {e}", file=sys.stderr)
            return False

    def close(self):
        """Close the database connection."""
        if self.conn:
            self.conn.close()

    def parse_bday(self, vcard_data):
        """
        Extract birthday from vCard data.
        Returns tuple of (date_string, has_year) or None if not found.
        
        Supports formats:
        - BDAY:1985-04-15
        - BDAY:--04-15
        - BDAY;VALUE=DATE:1985-04-15
        """
        if not vcard_data:
            return None
        
        # Match BDAY field (with or without VALUE parameter)
        pattern = r'BDAY(?:;[^:]*)?:([^\r\n]+)'
        match = re.search(pattern, vcard_data)
        
        if not match:
            return None
        
        bday_value = match.group(1).strip()
        
        # Check for year-less format (--MM-DD)
        if bday_value.startswith('--'):
            # Year-less format
            try:
                month, day = bday_value[2:].split('-')
                return (f"--{month}-{day}", False)
            except (ValueError, IndexError):
                return None
        else:
            # Full date format (YYYY-MM-DD or YYYYMMDD)
            if '-' in bday_value:
                # ISO format: YYYY-MM-DD
                try:
                    datetime.strptime(bday_value[:10], '%Y-%m-%d')
                    return (bday_value[:10], True)
                except ValueError:
                    return None
            elif len(bday_value) == 8:
                # Compact format: YYYYMMDD
                try:
                    dt = datetime.strptime(bday_value, '%Y%m%d')
                    return (dt.strftime('%Y-%m-%d'), True)
                except ValueError:
                    return None
        
        return None

    def extract_name(self, vcard_data):
        """Extract display name from vCard (FN field)."""
        if not vcard_data:
            return "Unknown"
        
        pattern = r'FN:([^\r\n]+)'
        match = re.search(pattern, vcard_data)
        
        if match:
            return match.group(1).strip()
        
        # Fallback to N field if FN not found
        pattern = r'N:([^;]+);([^;\r\n]*)'
        match = re.search(pattern, vcard_data)
        if match:
            last_name = match.group(1).strip()
            first_name = match.group(2).strip()
            return f"{first_name} {last_name}".strip()
        
        return "Unknown"

    def get_or_create_birthday_calendar(self, principal_uri):
        """
        Get or create the birthday calendar.
        
        Uses 'birthdays' as the calendar URI and 'Birthdays' as the display name.
        These are standard values, but could be made configurable if needed.
        """
        cursor = self.conn.cursor()
        
        # Check if birthday calendar exists
        cursor.execute("""
            SELECT id FROM calendars 
            WHERE principaluri = ? AND uri = 'birthdays'
        """, (principal_uri,))
        
        row = cursor.fetchone()
        if row:
            self.birthday_calendar_id = row['id']
            return self.birthday_calendar_id
        
        # Create birthday calendar
        cursor.execute("""
            INSERT INTO calendars (principaluri, uri, displayname, description, components, timezone)
            VALUES (?, 'birthdays', 'Birthdays', 'Auto-generated birthday calendar', 'VEVENT', 'UTC')
        """, (principal_uri,))
        
        self.conn.commit()
        self.birthday_calendar_id = cursor.lastrowid
        return self.birthday_calendar_id

    def create_birthday_event(self, name, bday_date, has_year):
        """
        Create iCalendar VEVENT for a birthday.
        
        Args:
            name: Person's name
            bday_date: Birthday date (YYYY-MM-DD or --MM-DD)
            has_year: Whether the birthday includes a birth year
        """
        # Generate unique UID based on name and date
        uid = hashlib.md5(f"birthday-{name}-{bday_date}".encode()).hexdigest()
        
        if has_year:
            # Full date with year
            year, month, day = bday_date.split('-')
            start_date = f"{year}{month}{day}"
            # Calculate age in summary
            summary = f"{name}'s Birthday"
        else:
            # Year-less format: use current year as starting point
            month, day = bday_date[2:].split('-')
            current_year = datetime.now().year
            start_date = f"{current_year}{month}{day}"
            summary = f"{name}'s Birthday"
        
        # Create VEVENT with annual recurrence
        vevent = f"""BEGIN:VCALENDAR
VERSION:2.0
PRODID:-//Baikal//Birthday Sync//EN
BEGIN:VEVENT
UID:{uid}@baikal
DTSTART;VALUE=DATE:{start_date}
SUMMARY:{summary}
RRULE:FREQ=YEARLY
TRANSP:TRANSPARENT
END:VEVENT
END:VCALENDAR
"""
        return vevent.strip()

    def sync_birthdays(self):
        """Main sync function: read contacts and update birthday calendar."""
        if not self.connect():
            return False
        
        cursor = self.conn.cursor()
        
        # Get all principals
        cursor.execute("SELECT uri FROM principals WHERE uri LIKE 'principals/%'")
        principals = cursor.fetchall()
        
        if not principals:
            print("No principals found", file=sys.stderr)
            return False
        
        # Use first principal for now (typically the admin user)
        # NOTE: This currently only syncs birthdays for the first principal.
        # To support multiple users, iterate through all principals.
        if len(principals) > 1:
            print(f"Found {len(principals)} principals. Using only the first one: {principals[0]['uri']}")
            print("To sync birthdays for all users, the script would need to be enhanced to iterate through all principals.")
        
        principal_uri = principals[0]['uri']
        
        # Get or create birthday calendar
        calendar_id = self.get_or_create_birthday_calendar(principal_uri)
        
        # Get all contacts with birthday info
        cursor.execute("""
            SELECT addressbookid, carddata, uri
            FROM cards
            WHERE carddata LIKE '%BDAY%'
        """)
        
        contacts = cursor.fetchall()
        
        if not contacts:
            print("No contacts with birthdays found")
            return True
        
        # Get existing birthday events to check for updates
        cursor.execute("""
            SELECT uri, calendardata FROM calendarobjects
            WHERE calendarid = ?
        """, (calendar_id,))
        
        existing_events = {row['uri']: row['calendardata'] for row in cursor.fetchall()}
        
        # Process each contact
        birthday_count = 0
        processed_uris = set()
        
        for contact in contacts:
            vcard_data = contact['carddata']
            
            bday_info = self.parse_bday(vcard_data)
            if not bday_info:
                continue
            
            bday_date, has_year = bday_info
            name = self.extract_name(vcard_data)
            
            # Create birthday event
            ical_data = self.create_birthday_event(name, bday_date, has_year)
            
            # Generate unique URI for the event
            event_uri = hashlib.md5(f"birthday-{name}-{bday_date}".encode()).hexdigest() + ".ics"
            processed_uris.add(event_uri)
            
            # Check if event exists and needs updating
            if event_uri in existing_events:
                if existing_events[event_uri] != ical_data:
                    # Update existing event
                    cursor.execute("""
                        UPDATE calendarobjects
                        SET calendardata = ?, lastmodified = ?, etag = ?, size = ?
                        WHERE calendarid = ? AND uri = ?
                    """, (
                        ical_data,
                        int(datetime.now().timestamp()),
                        hashlib.md5(ical_data.encode()).hexdigest(),
                        len(ical_data),
                        calendar_id,
                        event_uri
                    ))
            else:
                # Insert new event
                # Note: Baikal's schema uses 'firstoccurence' and 'lastoccurence' (not 'occurrence')
                cursor.execute("""
                    INSERT INTO calendarobjects (calendarid, uri, calendardata, lastmodified, etag, size, componenttype, firstoccurence, lastoccurence)
                    VALUES (?, ?, ?, ?, ?, ?, 'VEVENT', ?, ?)
                """, (
                    calendar_id,
                    event_uri,
                    ical_data,
                    int(datetime.now().timestamp()),
                    hashlib.md5(ical_data.encode()).hexdigest(),
                    len(ical_data),
                    int(datetime.now().timestamp()),
                    2147483647  # Max int32 for recurring events
                ))
            
            birthday_count += 1
        
        # Remove events for contacts that no longer have birthdays
        stale_uris = set(existing_events.keys()) - processed_uris
        for stale_uri in stale_uris:
            cursor.execute("""
                DELETE FROM calendarobjects
                WHERE calendarid = ? AND uri = ?
            """, (calendar_id, stale_uri))
        
        self.conn.commit()
        print(f"Successfully synced {birthday_count} birthdays to calendar")
        
        return True


def main():
    """Main entry point."""
    db_path = "/opt/baikal/Specific/db/db.sqlite"
    
    # Allow custom db path as argument
    if len(sys.argv) > 1:
        db_path = sys.argv[1]
    
    sync = BaikalBirthdaySync(db_path)
    
    try:
        success = sync.sync_birthdays()
        sys.exit(0 if success else 1)
    finally:
        sync.close()


if __name__ == "__main__":
    main()
