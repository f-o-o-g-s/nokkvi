#!/usr/bin/env python3
"""
Toggle light mode in the Navidrome client state database.

This script reads the state.redb database, modifies the light_mode field
in the Queue JSON, and writes it back.
"""

import sys
import json
import struct
from pathlib import Path

# redb file format constants
MAGIC = b'redb'
PAGE_SIZE = 4096

def read_redb_state(db_path: Path) -> dict | None:
    """
    Read the queue state from state.redb.
    
    This is a simplified reader that looks for the JSON data in the redb file.
    The redb format stores data in pages, and we need to find the serialized JSON.
    """
    try:
        with open(db_path, 'rb') as f:
            content = f.read()
            
        # Look for JSON data - it should contain "songs" and "light_mode" fields
        # We'll search for the pattern that indicates a Queue JSON object
        start_markers = [b'{"songs":', b'{"songs":[']
        
        for marker in start_markers:
            pos = content.find(marker)
            if pos != -1:
                # Found potential JSON, now find the end
                # Count braces to find the complete JSON object
                brace_count = 0
                start_pos = pos
                in_string = False
                escape_next = False
                
                for i in range(pos, len(content)):
                    char = chr(content[i]) if content[i] < 128 else None
                    
                    if char is None:
                        continue
                        
                    if escape_next:
                        escape_next = False
                        continue
                        
                    if char == '\\':
                        escape_next = True
                        continue
                        
                    if char == '"':
                        in_string = not in_string
                        continue
                        
                    if not in_string:
                        if char == '{':
                            brace_count += 1
                        elif char == '}':
                            brace_count -= 1
                            if brace_count == 0:
                                # Found the end of the JSON object
                                json_bytes = content[start_pos:i+1]
                                try:
                                    return json.loads(json_bytes.decode('utf-8'))
                                except json.JSONDecodeError:
                                    continue
        
        return None
        
    except Exception as e:
        print(f"Error reading database: {e}", file=sys.stderr)
        return None


def write_redb_state(db_path: Path, state: dict) -> bool:
    """
    Write the modified queue state back to state.redb.
    
    This replaces the JSON data in place. Since redb uses fixed-size pages,
    we need to ensure the new JSON fits in the same space or handle resizing.
    """
    try:
        with open(db_path, 'rb') as f:
            content = bytearray(f.read())
        
        # Find the old JSON
        start_markers = [b'{"songs":', b'{"songs":[']
        
        for marker in start_markers:
            pos = content.find(marker)
            if pos != -1:
                # Found the JSON, find its end
                brace_count = 0
                start_pos = pos
                end_pos = None
                in_string = False
                escape_next = False
                
                for i in range(pos, len(content)):
                    char = chr(content[i]) if content[i] < 128 else None
                    
                    if char is None:
                        continue
                        
                    if escape_next:
                        escape_next = False
                        continue
                        
                    if char == '\\':
                        escape_next = True
                        continue
                        
                    if char == '"':
                        in_string = not in_string
                        continue
                        
                    if not in_string:
                        if char == '{':
                            brace_count += 1
                        elif char == '}':
                            brace_count -= 1
                            if brace_count == 0:
                                end_pos = i + 1
                                break
                
                if end_pos is None:
                    continue
                
                # Serialize the new state
                new_json = json.dumps(state, separators=(',', ':')).encode('utf-8')
                old_json_len = end_pos - start_pos
                new_json_len = len(new_json)
                
                # Replace the JSON in the content
                # We need to handle the length prefix that redb uses
                # The format is: [4-byte length][json data]
                
                # Check if there's a length prefix before the JSON
                if start_pos >= 4:
                    # Read the potential length prefix
                    potential_len = struct.unpack('<I', content[start_pos-4:start_pos])[0]
                    if potential_len == old_json_len:
                        # There's a length prefix, update it
                        new_len_bytes = struct.pack('<I', new_json_len)
                        content[start_pos-4:start_pos] = new_len_bytes
                
                # Replace the JSON data
                content[start_pos:end_pos] = new_json
                
                # Write back to file
                with open(db_path, 'wb') as f:
                    f.write(content)
                
                return True
        
        return False
        
    except Exception as e:
        print(f"Error writing database: {e}", file=sys.stderr)
        return False


def toggle_light_mode(mode: str) -> int:
    """Toggle light mode in the state database."""
    db_path = Path.home() / '.config' / 'nokkvi' / 'app.redb'
    
    if not db_path.exists():
        print(f"Database not found: {db_path}", file=sys.stderr)
        return 1
    
    # Read current state
    state = read_redb_state(db_path)
    if state is None:
        print("Failed to read state from database", file=sys.stderr)
        return 1
    
    # Update light_mode
    if mode.lower() in ['true', '1', 'yes', 'light']:
        state['light_mode'] = True
        mode_str = "light"
    else:
        state['light_mode'] = False
        mode_str = "dark"
    
    # Write back
    if write_redb_state(db_path, state):
        print(f"✓ Set mode to: {mode_str}")
        return 0
    else:
        print("Failed to write state to database", file=sys.stderr)
        return 1


if __name__ == '__main__':
    if len(sys.argv) != 2:
        print(f"Usage: {sys.argv[0]} <true|false>", file=sys.stderr)
        print("  true  - Enable light mode", file=sys.stderr)
        print("  false - Enable dark mode", file=sys.stderr)
        sys.exit(1)
    
    sys.exit(toggle_light_mode(sys.argv[1]))
